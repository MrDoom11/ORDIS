//! Agente nativo: conversa com o Qwen pelo TabbyAPI, executa as ferramentas
//! pedidas e guarda as sessões em runtime/agente/sessoes.
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::controle::{self, Caminhos};
use crate::ferramentas::{self, Contexto};
use crate::modelos;

const RODADAS_MAX: usize = 12;
/// Estimativa grosseira para português e JSON; o Tabby recusa se passar do limite.
const CARACTERES_POR_TOKEN: usize = 3;
const TOKENS_POR_IMAGEM: usize = 1_200;

#[derive(Serialize, Deserialize, Clone)]
pub struct Chamada {
    pub id: String,
    pub nome: String,
    pub argumentos: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Mensagem {
    /// "user", "assistant" ou "tool".
    pub papel: String,
    #[serde(default)]
    pub texto: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raciocinio: Option<String>,
    /// Caminhos absolutos de imagens anexadas, lidas ou geradas.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub imagens: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chamadas: Vec<Chamada>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_chamada: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ferramenta: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub erro: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Sessao {
    pub id: String,
    pub titulo: String,
    pub criada: u64,
    pub atualizada: u64,
    pub mensagens: Vec<Mensagem>,
}

#[derive(Serialize)]
pub struct Resumo {
    pub id: String,
    pub titulo: String,
    pub atualizada: u64,
}

fn agora() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn pasta_sessoes(c: &Caminhos) -> PathBuf {
    c.runtime.join("agente/sessoes")
}

fn arquivo(c: &Caminhos, id: &str) -> Result<PathBuf, String> {
    if id.is_empty() || !id.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-') {
        return Err("Identificador de sessão inválido.".into());
    }
    Ok(pasta_sessoes(c).join(format!("{id}.json")))
}

pub fn listar(c: &Caminhos) -> Vec<Resumo> {
    let mut lista: Vec<Resumo> = fs::read_dir(pasta_sessoes(c))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| fs::read_to_string(e.path()).ok())
        .filter_map(|t| serde_json::from_str::<Sessao>(&t).ok())
        .map(|s| Resumo { id: s.id, titulo: s.titulo, atualizada: s.atualizada })
        .collect();
    lista.sort_by(|a, b| b.atualizada.cmp(&a.atualizada));
    lista
}

pub fn abrir(c: &Caminhos, id: &str) -> Result<Sessao, String> {
    let texto = fs::read_to_string(arquivo(c, id)?).map_err(|_| "Sessão não encontrada.".to_string())?;
    serde_json::from_str(&texto).map_err(|e| format!("Sessão corrompida: {e}"))
}

pub fn nova(c: &Caminhos) -> Result<Sessao, String> {
    let s = Sessao {
        id: controle::sufixo_unico(),
        titulo: "Nova conversa".into(),
        criada: agora(),
        atualizada: agora(),
        mensagens: vec![],
    };
    salvar(c, &s)?;
    Ok(s)
}

pub fn apagar(c: &Caminhos, id: &str) -> Result<(), String> {
    fs::remove_file(arquivo(c, id)?).map_err(|e| format!("Falha ao apagar a sessão: {e}"))
}

fn salvar(c: &Caminhos, s: &Sessao) -> Result<(), String> {
    fs::create_dir_all(pasta_sessoes(c)).map_err(|e| e.to_string())?;
    let destino = arquivo(c, &s.id)?;
    let tmp = destino.with_extension("tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(s).map_err(|e| e.to_string())?)
        .map_err(|e| format!("Falha ao salvar a sessão: {e}"))?;
    fs::rename(&tmp, &destino).map_err(|e| format!("Falha ao salvar a sessão: {e}"))
}

pub fn data_url(caminho: &Path) -> Result<String, String> {
    let bytes = fs::read(caminho).map_err(|e| format!("Falha ao ler a imagem: {e}"))?;
    let ext = caminho.extension().and_then(|e| e.to_str()).unwrap_or("png").to_ascii_lowercase();
    let mime = match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "image/png",
    };
    Ok(format!("data:{mime};base64,{}", B64.encode(bytes)))
}

/// Grava no workspace uma imagem enviada pela interface como data URL.
fn salvar_anexo(c: &Caminhos, url: &str) -> Result<PathBuf, String> {
    let (cabecalho, dados) = url.split_once(',').ok_or("Anexo inválido.")?;
    let ext = match cabecalho {
        h if h.contains("image/jpeg") => "jpg",
        h if h.contains("image/webp") => "webp",
        h if h.contains("image/gif") => "gif",
        h if h.contains("image/png") => "png",
        _ => return Err("Só imagens PNG, JPEG, WebP ou GIF podem ser anexadas.".into()),
    };
    let bytes = B64.decode(dados).map_err(|_| "Anexo inválido.")?;
    let pasta = ferramentas::workspace(c).join("anexos");
    fs::create_dir_all(&pasta).map_err(|e| e.to_string())?;
    let destino = pasta.join(format!("{}.{ext}", controle::sufixo_unico()));
    fs::write(&destino, bytes).map_err(|e| format!("Falha ao salvar o anexo: {e}"))?;
    Ok(destino)
}

/// `AGENTS.md` traz as regras gerais (versionado); `AGENTS.local.md`, se
/// existir, acrescenta o que é pessoal, como o nome do usuário (fora do Git).
fn prompt_sistema(c: &Caminhos) -> String {
    let ler = |nome: &str| fs::read_to_string(ferramentas::workspace(c).join(nome)).unwrap_or_default();
    let base = [ler("AGENTS.md"), ler("AGENTS.local.md")]
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    format!(
        "{}\n\nData e hora atuais: {}. Use-as como referência para \"hoje\", prazos e versões recentes.\nO workspace do agente fica em {}.",
        base,
        ferramentas::agora_formatado(),
        ferramentas::workspace(c).display()
    )
}

fn conteudo_usuario(texto: &str, imagens: &[String], enviar_imagens: bool) -> Value {
    if !enviar_imagens || imagens.is_empty() {
        let mut t = texto.to_string();
        for img in imagens {
            t.push_str(&format!("\n[Imagem anexada anteriormente: {img}]"));
        }
        return json!(t);
    }
    let mut partes = vec![json!({"type": "text", "text": if texto.is_empty() { "Analise a imagem." } else { texto }})];
    for img in imagens {
        if let Ok(url) = data_url(Path::new(img)) {
            partes.push(json!({"type": "image_url", "image_url": {"url": url}}));
        }
    }
    json!(partes)
}

/// Converte a sessão para o formato da API, descartando turnos antigos se o
/// contexto estimado passar do orçamento. Só imagens do turno atual são enviadas.
fn mensagens_api(c: &Caminhos, s: &Sessao) -> (Vec<Value>, usize) {
    let cfg = modelos::conversa(c);
    let orcamento = (cfg.contexto as usize).saturating_sub(cfg.max_tokens as usize + 1_500);
    let sistema = prompt_sistema(c);
    let custo = |m: &Mensagem| {
        (m.texto.len() + m.chamadas.iter().map(|ch| ch.argumentos.len() + 40).sum::<usize>())
            / CARACTERES_POR_TOKEN
            + 8
    };
    let turnos: Vec<usize> =
        s.mensagens.iter().enumerate().filter(|(_, m)| m.papel == "user").map(|(i, _)| i).collect();
    let ultimo_usuario = turnos.last().copied().unwrap_or(0);
    let mut inicio = 0;
    for &t in &turnos {
        inicio = t;
        let total: usize = sistema.len() / CARACTERES_POR_TOKEN
            + s.mensagens[t..].iter().map(custo).sum::<usize>()
            + s.mensagens[ultimo_usuario..].iter().map(|m| m.imagens.len() * TOKENS_POR_IMAGEM).sum::<usize>();
        if total <= orcamento {
            break;
        }
    }

    let mut saida = vec![json!({"role": "system", "content": sistema})];
    let mut pendentes: Vec<String> = vec![];
    let mut estimado = sistema.len() / CARACTERES_POR_TOKEN;
    for (i, m) in s.mensagens.iter().enumerate().skip(inicio) {
        let atual = i >= ultimo_usuario;
        if m.papel != "tool" && !pendentes.is_empty() {
            saida.push(conteudo_imagens_lidas(&pendentes));
            pendentes.clear();
        }
        estimado += custo(m);
        match m.papel.as_str() {
            "user" => {
                if atual {
                    estimado += m.imagens.len() * TOKENS_POR_IMAGEM;
                }
                saida.push(json!({"role": "user", "content": conteudo_usuario(&m.texto, &m.imagens, atual && cfg.visao)}));
            }
            "assistant" => {
                let mut msg = json!({"role": "assistant", "content": m.texto});
                if !m.chamadas.is_empty() {
                    msg["tool_calls"] = m
                        .chamadas
                        .iter()
                        .map(|ch| json!({"id": ch.id, "type": "function", "function": {"name": ch.nome, "arguments": ch.argumentos}}))
                        .collect();
                }
                saida.push(msg);
            }
            _ => {
                saida.push(json!({"role": "tool", "tool_call_id": m.id_chamada, "content": m.texto}));
                // Só `read` devolve imagens ao modelo; as geradas apenas aparecem na tela.
                if atual && cfg.visao && m.ferramenta.as_deref() == Some("read") {
                    estimado += m.imagens.len() * TOKENS_POR_IMAGEM;
                    pendentes.extend(m.imagens.iter().cloned());
                }
            }
        }
    }
    if !pendentes.is_empty() {
        saida.push(conteudo_imagens_lidas(&pendentes));
    }
    (saida, estimado)
}

fn conteudo_imagens_lidas(imagens: &[String]) -> Value {
    let mut partes = vec![json!({"type": "text", "text": "Imagens lidas pela ferramenta read:"})];
    for img in imagens {
        if let Ok(url) = data_url(Path::new(img)) {
            partes.push(json!({"type": "image_url", "image_url": {"url": url}}));
        }
    }
    json!({"role": "user", "content": partes})
}

/// Eventos enviados à interface durante uma resposta.
pub trait Saida {
    fn evento(&self, tipo: &str, dados: Value);
}

struct Resposta {
    texto: String,
    raciocinio: String,
    chamadas: Vec<Chamada>,
}

fn transmitir(
    c: &Caminhos,
    mensagens: &[Value],
    cancelar: &AtomicBool,
    saida: &dyn Saida,
) -> Result<Resposta, String> {
    let chave = controle::chave_api(c).ok_or("Chave da API do TabbyAPI não encontrada.")?;
    let cfg = modelos::conversa(c);
    let agente = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(600))
        .build();
    let corpo = json!({
        "model": cfg.modelo,
        "messages": mensagens,
        "tools": ferramentas::definicoes(),
        "stream": true,
        "max_tokens": cfg.max_tokens,
    });
    let resposta = agente
        .post("http://127.0.0.1:5000/v1/chat/completions")
        .set("Authorization", &format!("Bearer {chave}"))
        .send_json(corpo)
        .map_err(|e| match e {
            ureq::Error::Status(code, r) => {
                let detalhe = r.into_string().unwrap_or_default();
                format!("O modelo recusou o pedido (HTTP {code}). {}", detalhe.chars().take(400).collect::<String>())
            }
            e => format!("Falha ao falar com o modelo: {e}"),
        })?;
    let mut r = Resposta { texto: String::new(), raciocinio: String::new(), chamadas: vec![] };
    for linha in BufReader::new(resposta.into_reader()).lines() {
        if cancelar.load(Ordering::Relaxed) {
            // Fechar a conexão faz o Tabby abandonar a geração.
            break;
        }
        let linha = linha.map_err(|e| format!("Conexão com o modelo interrompida: {e}"))?;
        let Some(dados) = linha.strip_prefix("data:").map(str::trim) else { continue };
        if dados == "[DONE]" {
            break;
        }
        let Ok(v) = serde_json::from_str::<Value>(dados) else { continue };
        let delta = &v["choices"][0]["delta"];
        if let Some(t) = delta["reasoning_content"].as_str().filter(|t| !t.is_empty()) {
            r.raciocinio.push_str(t);
            saida.evento("raciocinio", json!({"texto": t}));
        }
        if let Some(t) = delta["content"].as_str().filter(|t| !t.is_empty()) {
            r.texto.push_str(t);
            saida.evento("texto", json!({"texto": t}));
        }
        for ch in delta["tool_calls"].as_array().into_iter().flatten() {
            let indice = ch["index"].as_u64().unwrap_or(r.chamadas.len() as u64) as usize;
            while r.chamadas.len() <= indice {
                r.chamadas.push(Chamada { id: String::new(), nome: String::new(), argumentos: String::new() });
            }
            let alvo = &mut r.chamadas[indice];
            if let Some(id) = ch["id"].as_str() {
                alvo.id = id.to_string();
            }
            if let Some(n) = ch["function"]["name"].as_str() {
                alvo.nome = n.to_string();
            }
            if let Some(a) = ch["function"]["arguments"].as_str() {
                // O Tabby envia a chamada inteira de uma vez; outros servidores, em pedaços.
                if serde_json::from_str::<Value>(a).is_ok() {
                    alvo.argumentos = a.to_string();
                } else {
                    alvo.argumentos.push_str(a);
                }
            }
        }
    }
    for (i, ch) in r.chamadas.iter_mut().enumerate() {
        if ch.id.is_empty() {
            ch.id = format!("call_{}_{i}", controle::sufixo_unico());
        }
    }
    Ok(r)
}

/// Executa um turno completo: grava a pergunta, conversa com o modelo e roda
/// as ferramentas até ele responder sem pedir mais nenhuma.
pub fn responder(
    c: &Caminhos,
    id: &str,
    texto: String,
    anexos: Vec<String>,
    cancelar: &AtomicBool,
    saida: &dyn Saida,
) -> Result<(), String> {
    let mut s = abrir(c, id)?;
    let imagens = anexos
        .iter()
        .map(|a| salvar_anexo(c, a).map(|p| p.display().to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    if s.mensagens.is_empty() {
        let base = if texto.trim().is_empty() { "Imagem" } else { texto.trim() };
        s.titulo = base.chars().take(60).collect();
    }
    s.mensagens.push(Mensagem { papel: "user".into(), texto, imagens, ..Default::default() });
    s.atualizada = agora();
    salvar(c, &s)?;
    rodar(c, s, cancelar, saida)
}

/// Descarta tudo depois da última mensagem do usuário e responde de novo.
pub fn refazer(c: &Caminhos, id: &str, cancelar: &AtomicBool, saida: &dyn Saida) -> Result<(), String> {
    let mut s = abrir(c, id)?;
    let ultima = s.mensagens.iter().rposition(|m| m.papel == "user").ok_or("Não há pergunta para refazer.")?;
    s.mensagens.truncate(ultima + 1);
    s.atualizada = agora();
    salvar(c, &s)?;
    rodar(c, s, cancelar, saida)
}

pub fn renomear(c: &Caminhos, id: &str, titulo: &str) -> Result<(), String> {
    let titulo = titulo.trim();
    if titulo.is_empty() {
        return Err("O título não pode ficar vazio.".into());
    }
    let mut s = abrir(c, id)?;
    s.titulo = titulo.chars().take(80).collect();
    salvar(c, &s)
}

fn rodar(c: &Caminhos, mut s: Sessao, cancelar: &AtomicBool, saida: &dyn Saida) -> Result<(), String> {
    garantir_inferencia(c, saida)?;
    for _ in 0..RODADAS_MAX {
        if cancelar.load(Ordering::Relaxed) {
            break;
        }
        let (mensagens, estimado) = mensagens_api(c, &s);
        saida.evento("inicio", json!({}));
        let r = transmitir(c, &mensagens, cancelar, saida)?;
        let chamadas = r.chamadas.clone();
        s.mensagens.push(Mensagem {
            papel: "assistant".into(),
            texto: r.texto,
            raciocinio: (!r.raciocinio.is_empty()).then_some(r.raciocinio),
            chamadas: chamadas.clone(),
            ..Default::default()
        });
        s.atualizada = agora();
        salvar(c, &s)?;
        if chamadas.is_empty() || cancelar.load(Ordering::Relaxed) {
            return Ok(());
        }
        for ch in chamadas {
            saida.evento("ferramenta", json!({"id": ch.id, "nome": ch.nome, "argumentos": ch.argumentos}));
            let progresso = |m: &str| saida.evento("status", json!({"texto": m}));
            let ctx = Contexto { caminhos: c, cancelar, progresso: &progresso, tokens_conversa: estimado };
            let res = ferramentas::executar(&ctx, &ch.nome, &ch.argumentos);
            let imagens: Vec<String> = res.imagens.iter().map(|p| p.display().to_string()).collect();
            saida.evento(
                "resultado",
                json!({"id": ch.id, "texto": res.texto, "erro": res.erro, "imagens": imagens}),
            );
            s.mensagens.push(Mensagem {
                papel: "tool".into(),
                texto: res.texto,
                imagens,
                id_chamada: Some(ch.id),
                ferramenta: Some(ch.nome),
                erro: res.erro,
                ..Default::default()
            });
            salvar(c, &s)?;
            if ultima_foi_imagem(&s) {
                // A geração de imagens descarrega o Qwen e o restaura ao terminar.
                garantir_inferencia(c, saida)?;
            }
        }
    }
    Ok(())
}

fn ultima_foi_imagem(s: &Sessao) -> bool {
    s.mensagens.last().and_then(|m| m.ferramenta.as_deref()) == Some("ordis_gerar_imagem")
}

fn garantir_inferencia(c: &Caminhos, saida: &dyn Saida) -> Result<(), String> {
    if controle::inferencia_pronta(c) {
        return Ok(());
    }
    saida.evento("status", json!({"texto": "Carregando o modelo..."}));
    controle::iniciar(c, &|m: &str| saida.evento("status", json!({"texto": m})))
}
