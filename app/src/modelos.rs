//! Modelos configuráveis pelo app.
//!
//! O de conversa continua definido no `config.yml` do TabbyAPI, que é a
//! fonte única de nome, contexto, formato de ferramentas, raciocínio, visão
//! e MTP (seção `draft_model`);
//! o app só lê e reescreve essas linhas. O limite por resposta e tudo do
//! modelo de imagem ficam em `runtime\modelos.json`. Sem esse arquivo (ou com
//! ele corrompido) valem os padrões de fábrica: Qwen3.8-27B e RealVisXL.
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::controle::{self, Caminhos};

const SAMPLERS: &[&str] = &[
    "euler", "euler_ancestral", "heun", "dpm_2", "dpm_2_ancestral", "lms", "dpmpp_2s_ancestral", "dpmpp_sde",
    "dpmpp_2m", "dpmpp_2m_sde", "dpmpp_3m_sde", "ddim", "uni_pc", "lcm",
];
const SCHEDULERS: &[&str] = &["normal", "karras", "exponential", "sgm_uniform", "simple", "ddim_uniform", "beta"];
/// Marcas aceitas num fluxo importado do ComfyUI (formato API).
pub const MARCAS: &[&str] =
    &["prompt", "negativo", "seed", "largura", "altura", "passos", "cfg", "sampler", "scheduler", "checkpoint"];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Conversa {
    /// Pasta em `runtime\tabbyAPI\models`.
    pub modelo: String,
    /// `max_seq_len` e `cache_size` do Tabby.
    pub contexto: u64,
    pub max_tokens: u32,
    /// `tool_format` do Tabby; vazio desliga a leitura de ferramentas.
    pub ferramentas: String,
    pub raciocinio: bool,
    pub visao: bool,
    /// Multi-Token Prediction: a camada extra do modelo adianta tokens que o
    /// modelo principal confere (`draft_mode: mtp`).
    pub mtp: bool,
    /// `draft_num_tokens`: quantos tokens o MTP adianta por passo.
    pub mtp_tokens: u32,
}

impl Default for Conversa {
    fn default() -> Self {
        Conversa {
            modelo: "Qwen3.8-27B-exl3-3.0bpw".into(),
            contexto: 32_768,
            max_tokens: 4096,
            ferramentas: "qwen3_5".into(),
            raciocinio: true,
            visao: true,
            mtp: false,
            mtp_tokens: 3,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Imagem {
    /// Arquivo em `runtime\ComfyUI\models\checkpoints`.
    pub checkpoint: String,
    pub passos: u64,
    pub cfg: f64,
    pub sampler: String,
    pub scheduler: String,
    /// Fluxo importado em `runtime\fluxos`; vazio usa o fluxo SDXL embutido.
    pub fluxo: String,
}

impl Default for Imagem {
    fn default() -> Self {
        Imagem {
            checkpoint: "RealVisXL_V5.0_fp16.safetensors".into(),
            passos: 30,
            cfg: 5.0,
            sampler: "dpmpp_sde".into(),
            scheduler: "karras".into(),
            fluxo: String::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Arquivo {
    max_tokens: Option<u32>,
    imagem: Imagem,
}

fn arquivo_app(c: &Caminhos) -> PathBuf {
    c.runtime.join("modelos.json")
}

fn config_tabby(c: &Caminhos) -> PathBuf {
    c.runtime.join("tabbyAPI/config.yml")
}

pub fn pasta_fluxos(c: &Caminhos) -> PathBuf {
    c.runtime.join("fluxos")
}

fn pasta_llm(c: &Caminhos) -> PathBuf {
    c.runtime.join("tabbyAPI/models")
}

fn pasta_checkpoints(c: &Caminhos) -> PathBuf {
    c.runtime.join("ComfyUI/models/checkpoints")
}

fn ler_arquivo(c: &Caminhos) -> Arquivo {
    fs::read_to_string(arquivo_app(c)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// Pares `chave: valor` diretamente sob uma seção do config.yml (recuo de 2).
fn chaves_secao(texto: &str, nome: &str) -> Vec<(String, String)> {
    let mut secao = "";
    let mut saida = vec![];
    for linha in texto.lines() {
        if !linha.starts_with(' ') && linha.trim_end().ends_with(':') {
            secao = linha.trim_end().trim_end_matches(':');
            continue;
        }
        if secao != nome || !linha.starts_with("  ") || linha.starts_with("   ") {
            continue;
        }
        if let Some((k, v)) = linha.trim().split_once(':') {
            let v = v.split(" #").next().unwrap_or("").trim().trim_matches(['"', '\'']);
            saida.push((k.trim().to_string(), v.to_string()));
        }
    }
    saida
}

pub fn conversa(c: &Caminhos) -> Conversa {
    let mut v = Conversa::default();
    if let Some(m) = ler_arquivo(c).max_tokens {
        v.max_tokens = m;
    }
    let Ok(texto) = fs::read_to_string(config_tabby(c)) else { return v };
    let chaves = chaves_secao(&texto, "model");
    let valor = |k: &str| chaves.iter().find(|(x, _)| x == k).map(|(_, v)| v.as_str());
    let rascunho = chaves_secao(&texto, "draft_model");
    let valor_rascunho = |k: &str| rascunho.iter().find(|(x, _)| x == k).map(|(_, v)| v.as_str());
    v.mtp = valor_rascunho("draft_mode") == Some("mtp");
    if let Some(n) = valor_rascunho("draft_num_tokens").and_then(|n| n.parse().ok()) {
        v.mtp_tokens = n;
    }
    if let Some(m) = valor("model_name").filter(|m| !m.is_empty()) {
        v.modelo = m.into();
    }
    if let Some(n) = valor("max_seq_len").and_then(|n| n.parse().ok()) {
        v.contexto = n;
    }
    // Chaves ausentes valem o padrão do Tabby, não o do ORDIS.
    v.ferramentas = valor("tool_format").unwrap_or("").into();
    v.raciocinio = valor("reasoning") == Some("true");
    v.visao = valor("vision") == Some("true");
    v.max_tokens = v.max_tokens.min((v.contexto / 2) as u32);
    v
}

pub fn imagem(c: &Caminhos) -> Imagem {
    ler_arquivo(c).imagem
}

/// Nome curto para a interface: "Qwen3.8-27B-exl3-3.0bpw" vira "Qwen3.8-27B".
pub fn nome_curto_conversa(modelo: &str) -> String {
    let minusculo = modelo.to_lowercase();
    let corte = ["-exl", "_exl", "-gguf", "-awq", "-gptq"].iter().filter_map(|s| minusculo.find(s)).min();
    modelo[..corte.unwrap_or(modelo.len())].to_string()
}

pub fn nome_curto_imagem(i: &Imagem) -> String {
    if !i.fluxo.is_empty() {
        return i.fluxo.trim_end_matches(".json").to_string();
    }
    let base = i.checkpoint.rsplit_once('.').map_or(i.checkpoint.as_str(), |(b, _)| b);
    let mut nome = base.to_string();
    for sufixo in ["_fp16", "-fp16", "_fp8", "-fp8", "_bf16", "-bf16"] {
        if let Some(n) = nome.strip_suffix(sufixo) {
            nome = n.to_string();
        }
    }
    nome
}

pub fn nomes(c: &Caminhos) -> Value {
    json!({"conversa": nome_curto_conversa(&conversa(c).modelo), "imagem": nome_curto_imagem(&imagem(c))})
}

// ---------- Opções disponíveis ----------

fn tamanho(p: &Path, filtro: &dyn Fn(&Path) -> bool) -> u64 {
    fs::read_dir(p)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && filtro(p))
        .filter_map(|p| p.metadata().ok())
        .map(|m| m.len())
        .sum()
}

fn gb(bytes: u64) -> f64 {
    (bytes as f64 / 1_073_741_824.0 * 10.0).round() / 10.0
}

/// Formatos de ferramenta que este Tabby conhece, lidos do próprio código dele.
fn formatos_ferramentas(c: &Caminhos) -> Vec<String> {
    let texto = fs::read_to_string(c.runtime.join("tabbyAPI/endpoints/OAI/utils/tools.py")).unwrap_or_default();
    let mut lista = vec![];
    if let Some(i) = texto.find("ALL_TOOLCALL_FORMATS") {
        let bloco = &texto[i..];
        let fim = bloco.find('}').unwrap_or(bloco.len());
        for parte in bloco[..fim].split('"').skip(1).step_by(2) {
            lista.push(parte.to_string());
        }
    }
    if lista.is_empty() {
        lista.push(Conversa::default().ferramentas);
    }
    lista
}

fn vram_total_gb() -> Option<f64> {
    // nvidia-smi: "usado MiB, total MiB"
    let v = controle::vram()?;
    let total: f64 = v.split(',').nth(1)?.trim().trim_end_matches("MiB").trim().parse().ok()?;
    Some((total / 1024.0 * 10.0).round() / 10.0)
}

pub fn opcoes(c: &Caminhos) -> Value {
    let mut llm: Vec<Value> = fs::read_dir(pasta_llm(c))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("config.json").is_file())
        .map(|p| {
            let config: Value = fs::read_to_string(p.join("config.json"))
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or(Value::Null);
            let visao = !config["vision_config"].is_null() || p.join("preprocessor_config.json").is_file();
            let camadas_mtp = |c: &Value| c["mtp_num_hidden_layers"].as_u64().or(c["num_nextn_predict_layers"].as_u64());
            let mtp = camadas_mtp(&config).or_else(|| camadas_mtp(&config["text_config"])).unwrap_or(0) > 0;
            let bytes = tamanho(&p, &|f| f.extension().is_some_and(|x| x == "safetensors"));
            json!({"nome": p.file_name().unwrap().to_string_lossy(), "gb": gb(bytes), "visao": visao, "mtp": mtp})
        })
        .collect();
    llm.sort_by(|a, b| a["nome"].as_str().cmp(&b["nome"].as_str()));
    let mut checkpoints: Vec<Value> = fs::read_dir(pasta_checkpoints(c))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| ["safetensors", "ckpt", "sft"].contains(&&*x.to_string_lossy())))
        .map(|p| {
            json!({"nome": p.file_name().unwrap().to_string_lossy(),
                   "gb": gb(p.metadata().map(|m| m.len()).unwrap_or(0))})
        })
        .collect();
    checkpoints.sort_by(|a, b| a["nome"].as_str().cmp(&b["nome"].as_str()));
    let mut fluxos: Vec<String> = fs::read_dir(pasta_fluxos(c))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".json"))
        .collect();
    fluxos.sort();
    json!({
        "conversa": conversa(c),
        "imagem": imagem(c),
        "padrao": {"conversa": Conversa::default(), "imagem": Imagem::default()},
        "modelos_conversa": llm,
        "checkpoints": checkpoints,
        "fluxos": fluxos,
        "ferramentas": formatos_ferramentas(c),
        "samplers": SAMPLERS,
        "schedulers": SCHEDULERS,
        "marcas": MARCAS,
        "vram_gb": vram_total_gb(),
        "pastas": {"conversa": pasta_llm(c), "imagem": pasta_checkpoints(c), "fluxos": pasta_fluxos(c)},
    })
}

// ---------- Gravação ----------

fn validar(c: &Caminhos, conversa: &Conversa, imagem: &Imagem) -> Result<(), String> {
    if conversa.modelo.contains(['/', '\\']) || !pasta_llm(c).join(&conversa.modelo).join("config.json").is_file() {
        return Err(format!("Modelo de conversa não encontrado: {}", conversa.modelo));
    }
    if !(4096..=262_144).contains(&conversa.contexto) || conversa.contexto % 256 != 0 {
        return Err("O contexto deve ficar entre 4.096 e 262.144 tokens, em múltiplos de 256.".into());
    }
    if !(256..=(conversa.contexto / 2) as u32).contains(&conversa.max_tokens) {
        return Err("O limite por resposta deve ficar entre 256 tokens e metade do contexto.".into());
    }
    if !(1..=8).contains(&conversa.mtp_tokens) {
        return Err("Os tokens adiantados do MTP devem ficar entre 1 e 8.".into());
    }
    if !conversa.ferramentas.is_empty() && !formatos_ferramentas(c).contains(&conversa.ferramentas) {
        return Err(format!("Formato de ferramentas desconhecido: {}", conversa.ferramentas));
    }
    if imagem.fluxo.is_empty() {
        if imagem.checkpoint.contains(['/', '\\']) || !pasta_checkpoints(c).join(&imagem.checkpoint).is_file() {
            return Err(format!("Modelo de imagem não encontrado: {}", imagem.checkpoint));
        }
    } else if imagem.fluxo.contains(['/', '\\']) || !pasta_fluxos(c).join(&imagem.fluxo).is_file() {
        return Err(format!("Fluxo não encontrado: {}", imagem.fluxo));
    }
    if !(1..=50).contains(&imagem.passos) {
        return Err("Os passos devem ficar entre 1 e 50.".into());
    }
    if !(0.0..=30.0).contains(&imagem.cfg) {
        return Err("O cfg deve ficar entre 0 e 30.".into());
    }
    if !SAMPLERS.contains(&imagem.sampler.as_str()) || !SCHEDULERS.contains(&imagem.scheduler.as_str()) {
        return Err("Sampler ou scheduler desconhecido.".into());
    }
    Ok(())
}

/// Reescreve só as chaves do modelo de conversa no config.yml, preservando o
/// resto do arquivo (comentários, ordem e quebras de linha). O MTP vive na
/// seção `draft_model`, que é criada ao ligar e some ao desligar se ficar vazia.
fn ajustar_config(original: &str, v: &Conversa) -> String {
    let texto = ajustar_modelo(original, v);
    ajustar_rascunho(&texto, v)
}

fn ajustar_rascunho(original: &str, v: &Conversa) -> String {
    let quebra = if original.contains("\r\n") { "\r\n" } else { "\n" };
    let novas = if v.mtp {
        vec!["  draft_mode: mtp".to_string(), format!("  draft_num_tokens: {}", v.mtp_tokens)]
    } else {
        vec![]
    };
    let mut secao = String::new();
    let mut saida: Vec<String> = vec![];
    let mut cabecalho: Option<usize> = None;
    for linha in original.lines() {
        if !linha.starts_with(' ') && linha.trim_end().ends_with(':') {
            secao = linha.trim_end().trim_end_matches(':').to_string();
            saida.push(linha.to_string());
            if secao == "draft_model" {
                cabecalho = Some(saida.len() - 1);
            }
            continue;
        }
        let direto = secao == "draft_model" && linha.starts_with("  ") && !linha.starts_with("   ");
        let chave = linha.trim().split(':').next().unwrap_or("");
        if direto && (chave == "draft_mode" || chave == "draft_num_tokens") {
            continue;
        }
        saida.push(linha.to_string());
    }
    match cabecalho {
        Some(i) => {
            saida.splice(i + 1..i + 1, novas);
            // Seção que ficou só com o cabeçalho sai do arquivo.
            let vazia = saida.get(i + 1).is_none_or(|l| !l.starts_with(' ') || l.trim().is_empty());
            if vazia {
                saida.remove(i);
            }
        }
        None if !novas.is_empty() => {
            saida.push("draft_model:".into());
            saida.extend(novas);
        }
        None => {}
    }
    saida.join(quebra) + if original.ends_with('\n') || original.is_empty() { quebra } else { "" }
}

fn ajustar_modelo(original: &str, v: &Conversa) -> String {
    let valores = [
        ("model_name", v.modelo.clone()),
        ("max_seq_len", v.contexto.to_string()),
        ("cache_size", v.contexto.to_string()),
        ("tool_format", v.ferramentas.clone()),
        ("reasoning", v.raciocinio.to_string()),
        ("vision", v.visao.to_string()),
    ];
    let mut secao = String::new();
    let mut saida: Vec<String> = vec![];
    let mut vistas: Vec<&str> = vec![];
    let mut linha_model = None;
    for linha in original.lines() {
        if !linha.starts_with(' ') && linha.trim_end().ends_with(':') {
            secao = linha.trim_end().trim_end_matches(':').to_string();
            saida.push(linha.to_string());
            if secao == "model" {
                linha_model = Some(saida.len());
            }
            continue;
        }
        let direto = secao == "model" && linha.starts_with("  ") && !linha.starts_with("   ");
        let chave = linha.trim().split(':').next().unwrap_or("");
        match valores.iter().find(|(k, _)| direto && *k == chave) {
            Some((k, valor)) => {
                vistas.push(k);
                if !valor.is_empty() {
                    saida.push(format!("  {k}: {valor}"));
                }
            }
            None => saida.push(linha.to_string()),
        }
    }
    let faltando: Vec<String> = valores
        .iter()
        .filter(|(k, valor)| !vistas.contains(k) && !valor.is_empty())
        .map(|(k, valor)| format!("  {k}: {valor}"))
        .collect();
    match linha_model {
        Some(i) => {
            saida.splice(i..i, faltando);
        }
        None => {
            saida.push("model:".into());
            saida.extend(faltando);
        }
    }
    let quebra = if original.contains("\r\n") { "\r\n" } else { "\n" };
    saida.join(quebra) + if original.ends_with('\n') || original.is_empty() { quebra } else { "" }
}

/// Grava as duas configurações. Devolve true se o modelo de conversa mudou
/// (o Tabby precisa ser reiniciado para valer).
pub fn salvar(c: &Caminhos, conversa: &Conversa, imagem: &Imagem) -> Result<bool, String> {
    validar(c, conversa, imagem)?;
    controle::com_trava(c, false, || {
        let atual = self::conversa(c);
        let mudou_tabby = Conversa { max_tokens: 0, ..atual.clone() } != Conversa { max_tokens: 0, ..conversa.clone() };
        if mudou_tabby {
            let caminho = config_tabby(c);
            let original = fs::read_to_string(&caminho).map_err(|e| format!("Falha ao ler o config.yml: {e}"))?;
            let tmp = caminho.with_extension("yml.tmp");
            fs::write(&tmp, ajustar_config(&original, conversa)).map_err(|e| e.to_string())?;
            fs::rename(&tmp, &caminho).map_err(|e| format!("Falha ao gravar o config.yml: {e}"))?;
        }
        let arquivo = Arquivo { max_tokens: Some(conversa.max_tokens), imagem: imagem.clone() };
        let tmp = arquivo_app(c).with_extension("tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(&arquivo).unwrap()).map_err(|e| e.to_string())?;
        fs::rename(&tmp, arquivo_app(c)).map_err(|e| format!("Falha ao gravar modelos.json: {e}"))?;
        Ok(mudou_tabby)
    })
}

/// Importa um fluxo do ComfyUI exportado em formato API, com marcas como
/// "{{prompt}}" nos campos que o ORDIS deve preencher.
pub fn importar_fluxo(c: &Caminhos, nome: &str, conteudo: &str) -> Result<String, String> {
    let v: Value = serde_json::from_str(conteudo).map_err(|e| format!("O arquivo não é um JSON válido: {e}"))?;
    let nos = v.as_object().ok_or("Exporte o fluxo no formato API do ComfyUI (Export (API)).")?;
    if !nos.values().all(|n| n["class_type"].is_string()) {
        return Err("Exporte o fluxo no formato API do ComfyUI (Export (API)).".into());
    }
    if !nos.values().any(|n| n["class_type"] == "SaveImage") {
        return Err("O fluxo precisa de um nó SaveImage.".into());
    }
    if !conteudo.contains("{{prompt}}") {
        return Err("Coloque \"{{prompt}}\" no texto do prompt positivo do fluxo.".into());
    }
    let base: String = Path::new(nome)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .map(|ch| if ch.is_alphanumeric() || "-_. ".contains(ch) { ch } else { '_' })
        .collect();
    let base = if base.trim().is_empty() { "fluxo".to_string() } else { base.trim().to_string() };
    fs::create_dir_all(pasta_fluxos(c)).map_err(|e| e.to_string())?;
    let arquivo = format!("{base}.json");
    fs::write(pasta_fluxos(c).join(&arquivo), conteudo).map_err(|e| e.to_string())?;
    Ok(arquivo)
}

// ---------- Teste do modelo de conversa ----------

fn conversar(c: &Caminhos, corpo: Value) -> Result<Value, String> {
    let chave = controle::chave_api(c).ok_or("Chave da API do TabbyAPI não encontrada.")?;
    ureq::post("http://127.0.0.1:5000/v1/chat/completions")
        .timeout(std::time::Duration::from_secs(300))
        .set("Authorization", &format!("Bearer {chave}"))
        .send_json(corpo)
        .map_err(|e| match e {
            ureq::Error::Status(code, r) => {
                format!("HTTP {code}: {}", r.into_string().unwrap_or_default().chars().take(300).collect::<String>())
            }
            e => e.to_string(),
        })?
        .into_json()
        .map_err(|e| e.to_string())
}

/// Carrega o modelo configurado (se preciso) e confere resposta simples,
/// separação do raciocínio, chamada de ferramenta e, se ativada, visão.
pub fn testar_conversa(c: &Caminhos, progresso: &dyn Fn(&str)) -> Value {
    let cfg = conversa(c);
    let mut etapas = vec![];
    fn etapa(etapas: &mut Vec<Value>, nome: &str, inicio: std::time::Instant, r: Result<String, String>) -> bool {
        let ok = r.is_ok();
        let detalhe = r.unwrap_or_else(|e| e);
        etapas.push(json!({"nome": nome, "ok": ok, "detalhe": detalhe,
                           "segundos": (inicio.elapsed().as_secs_f64() * 10.0).round() / 10.0}));
        ok
    }

    let t = std::time::Instant::now();
    let carga = if controle::inferencia_pronta(c) {
        Ok("Já estava carregado.".to_string())
    } else {
        progresso("Carregando o modelo…");
        controle::iniciar(c, progresso).map(|_| "Carregado.".to_string())
    };
    if !etapa(&mut etapas, "Carregar o modelo", t, carga) {
        return json!({"ok": false, "etapas": etapas});
    }

    progresso("Testando uma resposta curta…");
    let t = std::time::Instant::now();
    let r = conversar(c, json!({"model": cfg.modelo, "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Responda apenas a palavra: OK"}]}))
    .and_then(|v| {
        let m = &v["choices"][0]["message"];
        let texto = m["content"].as_str().unwrap_or("").trim().to_string();
        let raciocinio = m["reasoning_content"].as_str().is_some_and(|s| !s.trim().is_empty());
        if texto.is_empty() {
            return Err("A resposta veio vazia.".into());
        }
        if texto.contains("</think>") || (cfg.raciocinio && !raciocinio && texto.len() > 40) {
            return Err(format!(
                "O raciocínio veio misturado com a resposta. Confira o formato de raciocínio do modelo. Início: {}",
                texto.chars().take(80).collect::<String>()
            ));
        }
        Ok(format!("Respondeu \"{}\"{}.", texto.chars().take(40).collect::<String>(),
                   if raciocinio { ", com o raciocínio separado" } else { "" }))
    });
    etapa(&mut etapas, "Resposta curta", t, r);

    if !cfg.ferramentas.is_empty() {
        progresso("Testando uma chamada de ferramenta…");
        let t = std::time::Instant::now();
        let r = conversar(c, json!({"model": cfg.modelo, "max_tokens": 2048,
            "messages": [{"role": "user", "content": "Use a ferramenta hora_atual para saber a hora. Não responda sem chamá-la."}],
            "tools": [{"type": "function", "function": {"name": "hora_atual", "description": "Devolve a hora atual.",
                        "parameters": {"type": "object", "properties": {}}}}]}))
        .and_then(|v| {
            let chamadas = &v["choices"][0]["message"]["tool_calls"];
            match chamadas[0]["function"]["name"].as_str() {
                Some("hora_atual") => Ok("Chamou a ferramenta corretamente.".into()),
                _ => Err(format!(
                    "O modelo não chamou a ferramenta. Confira o formato de ferramentas. Resposta: {}",
                    v["choices"][0]["message"]["content"].as_str().unwrap_or("").chars().take(120).collect::<String>()
                )),
            }
        });
        etapa(&mut etapas, "Chamada de ferramenta", t, r);
    }

    if cfg.visao {
        progresso("Testando a leitura de imagem…");
        let t = std::time::Instant::now();
        let r = crate::agente::data_url(&c.projeto.join("app/ui/ordis.png"))
            .and_then(|url| {
                conversar(c, json!({"model": cfg.modelo, "max_tokens": 2048, "messages": [{"role": "user", "content": [
                    {"type": "text", "text": "Qual é a cor predominante desta imagem? Responda com uma palavra em português."},
                    {"type": "image_url", "image_url": {"url": url}}]}]}))
            })
            .and_then(|v| {
                let texto = v["choices"][0]["message"]["content"].as_str().unwrap_or("").trim().to_lowercase();
                if texto.contains("azul") || texto.contains("blue") || texto.contains("ciano") {
                    Ok("Reconheceu a cor do logo do ORDIS.".into())
                } else {
                    Err(format!("Resposta inesperada para uma imagem azul: \"{}\".", texto.chars().take(60).collect::<String>()))
                }
            });
        etapa(&mut etapas, "Leitura de imagem", t, r);
    }
    let ok = etapas.iter().all(|e| e["ok"] == true);
    json!({"ok": ok, "etapas": etapas})
}

#[cfg(test)]
mod testes {
    use super::*;

    const CFG: &str = "network:\r\n  port: 5000\r\nmodel:\r\n  model_dir: models\r\n  model_name: Qwen\r\n  max_seq_len: 32768\r\n  cache_size: 32768\r\n  cache_mode: Q8\r\n  vision: true\r\n  reasoning: true\r\n  template_vars_default:\r\n    enable_thinking: true\r\n  tool_format: qwen3_5\r\nsampling:\r\n  override_preset: x\r\n";

    #[test]
    fn le_o_config_do_tabby() {
        let ch = chaves_secao(CFG, "model");
        assert!(ch.contains(&("model_name".into(), "Qwen".into())));
        assert!(!ch.iter().any(|(k, _)| k == "enable_thinking"));
    }

    #[test]
    fn reescreve_so_as_chaves_do_modelo() {
        let v = Conversa {
            modelo: "Outro".into(),
            contexto: 16384,
            max_tokens: 2048,
            ferramentas: String::new(),
            raciocinio: false,
            visao: true,
            mtp: false,
            mtp_tokens: 3,
        };
        let novo = ajustar_config(CFG, &v);
        assert_eq!(novo, CFG
            .replace("model_name: Qwen", "model_name: Outro")
            .replace("32768", "16384")
            .replace("  reasoning: true\r\n", "  reasoning: false\r\n")
            .replace("  tool_format: qwen3_5\r\n", ""));
        // Com os mesmos valores, o arquivo volta byte a byte.
        let original = Conversa { modelo: "Qwen".into(), contexto: 32768, ferramentas: "qwen3_5".into(), ..v };
        let original = Conversa { raciocinio: true, ..original };
        assert_eq!(ajustar_config(CFG, &original), CFG);
    }

    #[test]
    fn liga_e_desliga_o_mtp_sem_sobras() {
        let ligado = Conversa { modelo: "Qwen".into(), mtp: true, ..Conversa::default() };
        let com = ajustar_config(CFG, &Conversa { mtp_tokens: 4, ..ligado.clone() });
        assert!(com.ends_with("sampling:\r\n  override_preset: x\r\ndraft_model:\r\n  draft_mode: mtp\r\n  draft_num_tokens: 4\r\n"));
        let lido = chaves_secao(&com, "draft_model");
        assert_eq!(lido, vec![("draft_mode".into(), "mtp".into()), ("draft_num_tokens".into(), "4".into())]);
        // Trocar os tokens não duplica a seção.
        let de_novo = ajustar_config(&com, &Conversa { mtp_tokens: 3, ..ligado.clone() });
        assert_eq!(de_novo.matches("draft_model:").count(), 1);
        assert!(de_novo.contains("  draft_num_tokens: 3\r\n"));
        // Desligar remove a seção que o ORDIS criou.
        let sem = ajustar_config(&de_novo, &Conversa { mtp: false, ..ligado.clone() });
        assert!(!sem.contains("draft"));
        // Outras opções da seção são preservadas.
        let outra = format!("{CFG}draft_model:\r\n  draft_cache_mode: Q8\r\n");
        let sem_outra = ajustar_config(&ajustar_config(&outra, &ligado), &Conversa { mtp: false, ..ligado });
        assert!(sem_outra.ends_with("draft_model:\r\n  draft_cache_mode: Q8\r\n"));
    }

    #[test]
    fn acrescenta_chaves_ausentes() {
        let novo = ajustar_config("model:\n  model_name: A\n", &Conversa::default());
        assert!(novo.starts_with("model:\n  max_seq_len: 32768\n"));
        assert!(novo.contains("  model_name: Qwen3.8-27B-exl3-3.0bpw\n"));
        assert!(novo.contains("  tool_format: qwen3_5\n"));
    }

    #[test]
    fn nomes_curtos() {
        assert_eq!(nome_curto_conversa("Qwen3.8-27B-exl3-3.0bpw"), "Qwen3.8-27B");
        assert_eq!(nome_curto_imagem(&Imagem::default()), "RealVisXL_V5.0");
    }
}
