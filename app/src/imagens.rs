//! Geração local de imagens com o ComfyUI e o modelo configurado em
//! `modelos.rs` (padrão: RealVisXL).
//!
//! A GPU não comporta o Qwen e o gerador juntos: a geração descarrega a
//! inferência, sobe o ComfyUI, gera, encerra o ComfyUI e restaura o Qwen se
//! ele estava ligado. Tudo sob a trava de controle.
use std::fs;
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use serde_json::{json, Value};
use sysinfo::System;

use crate::controle::{self, Caminhos};
use crate::modelos::{self, Imagem};

const BASE: &str = "http://127.0.0.1:8188";
const NEGATIVO_PADRAO: &str = "bad hands, bad anatomy, deformed, blurry, watermark, text";

pub fn pasta_saida(c: &Caminhos) -> PathBuf {
    c.projeto.join("agente/workspace/imagens")
}

struct Parametros {
    prompt: String,
    negative: String,
    width: u64,
    height: u64,
    steps: u64,
    seed: u64,
}

fn validar(v: &Value, passos_padrao: u64) -> Result<Parametros, String> {
    let prompt = v["prompt"].as_str().unwrap_or("");
    let tamanho = prompt.trim().chars().count();
    if !(1..=3000).contains(&tamanho) {
        return Err("O prompt deve ter de 1 a 3000 caracteres.".into());
    }
    let negative = match &v["negative"] {
        Value::Null => NEGATIVO_PADRAO,
        Value::String(s) if s.chars().count() <= 1500 => s.as_str(),
        _ => return Err("Prompt negativo inválido.".into()),
    };
    let inteiro = |nome: &str, padrao: u64, min: u64, max: u64| -> Result<u64, String> {
        match &v[nome] {
            Value::Null => Ok(padrao),
            n => n
                .as_u64()
                .filter(|x| (min..=max).contains(x))
                .ok_or_else(|| format!("{nome} fora dos limites.")),
        }
    };
    let aleatoria = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().subsec_nanos()
        as u64
        ^ u64::from_str_radix(&controle::sufixo_unico()[8..], 16).unwrap_or(0);
    let p = Parametros {
        prompt: prompt.to_string(),
        negative: negative.to_string(),
        width: inteiro("width", 1024, 512, 1536)?,
        height: inteiro("height", 1024, 512, 1536)?,
        steps: inteiro("steps", passos_padrao, 1, 50)?,
        seed: inteiro("seed", aleatoria & 0xffff_ffff, 0, u32::MAX as u64)?,
    };
    if p.width % 64 != 0 || p.height % 64 != 0 || p.width * p.height > 1_572_864 {
        return Err("A resolução deve ser múltipla de 64 e ter até 1,5 MP.".into());
    }
    Ok(p)
}

/// Fluxo SDXL embutido: checkpoint, dois prompts, KSampler, decodificação e saída.
fn workflow(p: &Parametros, id: &str, m: &Imagem) -> Value {
    json!({
        "1": {"class_type": "CheckpointLoaderSimple", "inputs": {"ckpt_name": m.checkpoint}},
        "2": {"class_type": "CLIPTextEncode", "inputs": {"text": p.prompt, "clip": ["1", 1]}},
        "3": {"class_type": "CLIPTextEncode", "inputs": {"text": p.negative, "clip": ["1", 1]}},
        "4": {"class_type": "EmptyLatentImage", "inputs": {"width": p.width, "height": p.height, "batch_size": 1}},
        "5": {"class_type": "KSampler", "inputs": {
            "model": ["1", 0], "positive": ["2", 0], "negative": ["3", 0], "latent_image": ["4", 0],
            "seed": p.seed, "steps": p.steps, "cfg": m.cfg, "sampler_name": m.sampler,
            "scheduler": m.scheduler, "denoise": 1.0}},
        "6": {"class_type": "VAEDecode", "inputs": {"samples": ["5", 0], "vae": ["1", 2]}},
        "7": {"class_type": "SaveImage", "inputs": {"images": ["6", 0], "filename_prefix": format!("ORDIS_{id}")}}
    })
}

/// Troca as marcas "{{nome}}" de um fluxo importado. Uma marca sozinha no
/// campo vira o valor com o tipo certo (número para seed, passos etc.).
fn preencher(v: &mut Value, valores: &[(&str, Value)]) {
    match v {
        Value::String(s) => {
            if let Some((_, valor)) = valores.iter().find(|(k, _)| *s == format!("{{{{{k}}}}}")) {
                *v = valor.clone();
            } else if s.contains("{{") {
                for (k, valor) in valores {
                    let texto = valor.as_str().map(str::to_string).unwrap_or_else(|| valor.to_string());
                    *s = s.replace(&format!("{{{{{k}}}}}"), &texto);
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| preencher(x, valores)),
        Value::Object(o) => o.values_mut().for_each(|x| preencher(x, valores)),
        _ => {}
    }
}

fn fluxo_importado(c: &Caminhos, p: &Parametros, id: &str, m: &Imagem) -> Result<Value, String> {
    let texto = fs::read_to_string(modelos::pasta_fluxos(c).join(&m.fluxo))
        .map_err(|e| format!("Falha ao ler o fluxo {}: {e}", m.fluxo))?;
    let mut grafo: Value = serde_json::from_str(&texto).map_err(|e| format!("Fluxo inválido: {e}"))?;
    preencher(&mut grafo, &[
        ("prompt", json!(p.prompt)), ("negativo", json!(p.negative)), ("seed", json!(p.seed)),
        ("largura", json!(p.width)), ("altura", json!(p.height)), ("passos", json!(p.steps)),
        ("cfg", json!(m.cfg)), ("sampler", json!(m.sampler)), ("scheduler", json!(m.scheduler)),
        ("checkpoint", json!(m.checkpoint)),
    ]);
    // A saída precisa do prefixo do ORDIS para cair na pasta certa.
    for no in grafo.as_object_mut().ok_or("Fluxo inválido.")?.values_mut() {
        if no["class_type"] == "SaveImage" {
            no["inputs"]["filename_prefix"] = json!(format!("ORDIS_{id}"));
        }
    }
    Ok(grafo)
}

/// Nós que o ORDIS acompanha: o amostrador (passos), a decodificação e a saída.
struct Nos {
    amostrador: String,
    decodificacao: String,
    saida: String,
}

fn nos(grafo: &Value) -> Result<Nos, String> {
    let mut lista: Vec<(&String, &Value)> = grafo.as_object().ok_or("Fluxo inválido.")?.iter().collect();
    // Ordem numérica dos ids, como o ComfyUI exporta.
    lista.sort_by_key(|(k, _)| (k.parse::<u64>().unwrap_or(u64::MAX), (*k).clone()));
    let achar = |f: &dyn Fn(&str) -> bool| {
        lista.iter().find(|(_, n)| n["class_type"].as_str().is_some_and(f)).map(|(k, _)| (*k).clone())
    };
    Ok(Nos {
        amostrador: achar(&|t| t.contains("Sampler") && !t.starts_with("KSamplerSelect")).unwrap_or_default(),
        decodificacao: achar(&|t| t.starts_with("VAEDecode")).unwrap_or_default(),
        saida: achar(&|t| t == "SaveImage").ok_or("O fluxo não tem um nó SaveImage.")?,
    })
}

/// Etapa e progresso publicados para a interface a cada mudança.
struct Status<'a> {
    dados: Value,
    publicar: &'a dyn Fn(&Value),
}

impl Status<'_> {
    fn etapa(&mut self, etapa: &str, mensagem: &str) {
        if self.dados["stage"] != etapa {
            self.dados["progress_value"] = Value::Null;
            self.dados["progress_max"] = Value::Null;
        }
        self.dados["stage"] = json!(etapa);
        self.mensagem(mensagem);
    }
    fn mensagem(&mut self, mensagem: &str) {
        self.dados["message"] = json!(mensagem);
        (self.publicar)(&self.dados);
    }
    fn passos(&mut self, valor: f64, maximo: f64) {
        // O ComfyUI repete cada passo em dois tipos de evento e manda 0/1 antes do KSampler.
        let atual = (self.dados["progress_value"].as_f64(), self.dados["progress_max"].as_f64());
        if maximo <= 1.0 || atual == (Some(valor), Some(maximo)) {
            return;
        }
        self.dados["progress_value"] = json!(valor);
        self.dados["progress_max"] = json!(maximo);
        self.mensagem(&format!("Gerando imagem: passo {valor} de {maximo}..."));
    }
}

#[derive(PartialEq, Clone, Copy)]
enum Parada {
    Local,
    Global,
}

enum Erro {
    Cancelado(Parada),
    Falha(String),
}

impl From<&str> for Erro {
    fn from(e: &str) -> Self {
        Erro::Falha(e.to_string())
    }
}

impl From<String> for Erro {
    fn from(e: String) -> Self {
        Erro::Falha(e)
    }
}

fn versao_marcador(c: &Caminhos) -> Option<SystemTime> {
    fs::metadata(controle::marcador_desligamento(c)).and_then(|m| m.modified()).ok()
}

fn pedir(metodo: &str, rota: &str, corpo: Option<Value>, limite: u64) -> Result<Value, ureq::Error> {
    let req = ureq::request(metodo, &format!("{BASE}{rota}")).timeout(Duration::from_secs(limite));
    let resp = match corpo {
        Some(c) => req.send_json(c)?,
        None => req.call()?,
    };
    Ok(resp.into_json().unwrap_or(Value::Null))
}

/// Escuta o WebSocket do ComfyUI para saber os passos reais do KSampler. O
/// histórico HTTP continua sendo a fonte do resultado; sem WebSocket só se
/// perde o detalhe do progresso.
fn ouvir_progresso(cliente: &str, parar: Arc<AtomicBool>) -> Receiver<Value> {
    let (tx, rx) = mpsc::sync_channel(64);
    let url = format!("ws://127.0.0.1:8188/ws?clientId={cliente}");
    thread::spawn(move || {
        let Ok(tcp) = TcpStream::connect_timeout(&"127.0.0.1:8188".parse().unwrap(), Duration::from_secs(3)) else {
            return;
        };
        let _ = tcp.set_read_timeout(Some(Duration::from_millis(500)));
        let Ok((mut ws, _)) = tungstenite::client(url.as_str(), tcp) else { return };
        while !parar.load(Ordering::Relaxed) {
            match ws.read() {
                Ok(tungstenite::Message::Text(t)) => {
                    if let Ok(v) = serde_json::from_str::<Value>(&t) {
                        if matches!(v["type"].as_str(), Some("progress" | "progress_state" | "executing")) {
                            let _ = tx.try_send(v);
                        }
                    }
                }
                Ok(tungstenite::Message::Close(_)) => break,
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                    if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(_) => break,
            }
        }
        let _ = ws.close(None);
    });
    rx
}

fn publicar_progresso(rx: &Receiver<Value>, prompt_id: &str, nos: &Nos, st: &mut Status) {
    while let Ok(ev) = rx.try_recv() {
        let mut dados = &ev["data"];
        if dados["prompt_id"] != prompt_id {
            continue;
        }
        match ev["type"].as_str() {
            Some("executing") => {
                match dados["node"].as_str() {
                    Some(n) if n == nos.decodificacao => st.mensagem("Decodificando a imagem..."),
                    Some(n) if n == nos.saida => st.mensagem("Salvando a imagem..."),
                    _ => {}
                }
                continue;
            }
            Some("progress_state") => dados = &dados["nodes"][nos.amostrador.as_str()],
            _ => {
                if !dados["node"].is_null() && dados["node"] != nos.amostrador.as_str() {
                    continue;
                }
            }
        }
        if let (Some(v), Some(m)) = (dados["value"].as_f64(), dados["max"].as_f64()) {
            if m > 0.0 && v.is_finite() && m.is_finite() {
                st.passos(v.clamp(0.0, m), m);
            }
        }
    }
}

/// Gera uma imagem. `cancelar` interrompe só este pedido; o Desligar do
/// ORDIS (marcador global) interrompe e impede a restauração do Qwen.
/// Devolve `{ok, file, seed, ...}` ou `{ok: false, error, cancelled}`.
pub fn gerar(c: &Caminhos, args: &Value, cancelar: &AtomicBool, ao_status: &dyn Fn(&Value)) -> Value {
    let inicio = Instant::now();
    let id = controle::sufixo_unico();
    let modelo = modelos::imagem(c);
    let base_global = versao_marcador(c);
    let global = || versao_marcador(c).is_some_and(|v| Some(v) != base_global);
    let checar = || -> Result<(), Erro> {
        if global() {
            Err(Erro::Cancelado(Parada::Global))
        } else if cancelar.load(Ordering::Relaxed) {
            Err(Erro::Cancelado(Parada::Local))
        } else {
            Ok(())
        }
    };
    let mut st = Status {
        dados: json!({"job_id": id, "stage": "preparing", "message": "Preparando a geração...",
                      "progress_value": null, "progress_max": null}),
        publicar: ao_status,
    };
    (st.publicar)(&st.dados);
    let mut aviso: Option<String> = None;

    let resultado = (|| -> Result<Value, Erro> {
        let p = validar(args, modelo.passos)?;
        let grafo = if modelo.fluxo.is_empty() {
            let checkpoint = c.runtime.join("ComfyUI/models/checkpoints").join(&modelo.checkpoint);
            if !checkpoint.is_file() {
                return Err(Erro::Falha(format!("Modelo de imagem não encontrado: {}", modelo.checkpoint)));
            }
            workflow(&p, &id, &modelo)
        } else {
            fluxo_importado(c, &p, &id, &modelo)?
        };
        let nos = nos(&grafo)?;
        let saida = pasta_saida(c);
        fs::create_dir_all(&saida).map_err(|e| e.to_string())?;
        controle::com_trava(c, false, || -> Result<Value, Erro> {
            checar()?;
            let mut estado = controle::ler_estado(c);
            let mut sys = System::new();
            let restaurar = estado.get("inferencia").is_some_and(|i| controle::rastreado(&mut sys, i));
            // Nunca encerra um processo externo que esteja usando as portas.
            for (nome, porta) in controle::SERVICOS {
                let nosso = estado.get(nome).is_some_and(|i| controle::rastreado(&mut sys, i));
                if controle::ocupada(porta) && !nosso {
                    return Err(Erro::Falha(format!("Porta de {nome} ocupada por processo externo.")));
                }
            }
            let mut prompt_id: Option<String> = None;
            let parar_ws = Arc::new(AtomicBool::new(false));
            let gerado = (|| -> Result<Value, Erro> {
                st.etapa("releasing_llm", "Liberando a GPU para imagens...");
                controle::parar_um(c, "inferencia", &mut estado, &mut sys)?;
                checar()?;
                st.etapa("starting_images", "Iniciando o gerador de imagens...");
                let iniciado = controle::iniciar_servico(c, "imagens", &mut estado, &mut sys, &|_| {}, &|| {
                    checar().map_err(|_| "cancelado".to_string())
                });
                checar()?;
                iniciado?;
                let cliente = format!("ordis-{id}");
                let rx = ouvir_progresso(&cliente, parar_ws.clone());
                st.etapa("generating", "Carregando o modelo de imagem...");
                let r = pedir("POST", "/prompt", Some(json!({"prompt": grafo.clone(), "client_id": cliente})), 20)
                    .map_err(|e| format!("O ComfyUI recusou o pedido: {e}"))?;
                let pid = r["prompt_id"].as_str().ok_or("O ComfyUI não devolveu um identificador.")?.to_string();
                prompt_id = Some(pid.clone());
                let limite = Instant::now() + Duration::from_secs(1200);
                let mut falhas = 0;
                while Instant::now() < limite {
                    checar()?;
                    if !estado.get("imagens").is_some_and(|i| controle::rastreado(&mut sys, i)) {
                        return Err(Erro::Falha("O ComfyUI encerrou durante a geração.".into()));
                    }
                    publicar_progresso(&rx, &pid, &nos, &mut st);
                    let historico = match pedir("GET", &format!("/history/{pid}"), None, 3) {
                        Ok(v) => {
                            falhas = 0;
                            v[pid.as_str()].clone()
                        }
                        Err(ureq::Error::Status(code, _)) => {
                            return Err(Erro::Falha(format!("O ComfyUI respondeu HTTP {code}.")));
                        }
                        Err(_) => {
                            falhas += 1;
                            if falhas >= 5 {
                                return Err(Erro::Falha("Gerador de imagens sem resposta após cinco tentativas.".into()));
                            }
                            st.mensagem("Aguardando resposta do gerador de imagens...");
                            thread::sleep(Duration::from_millis(250));
                            continue;
                        }
                    };
                    if historico.is_object() {
                        if historico["status"]["status_str"] == "error" {
                            let detalhe: String = historico["status"].to_string().chars().take(1500).collect();
                            return Err(Erro::Falha(format!("Falha na geração. Consulte imagens.log: {detalhe}")));
                        }
                        if let Some(img) = historico["outputs"][nos.saida.as_str()]["images"].get(0) {
                            let nome = img["filename"].as_str().ok_or("Saída sem nome de arquivo.")?;
                            let sub = img["subfolder"].as_str().unwrap_or("");
                            let arquivo = saida
                                .join(sub)
                                .join(nome)
                                .canonicalize()
                                .map_err(|_| "Arquivo de saída não encontrado.".to_string())?;
                            if !arquivo.starts_with(saida.canonicalize().map_err(|e| e.to_string())?) {
                                return Err(Erro::Falha("Arquivo de saída fora da pasta de imagens.".into()));
                            }
                            let parametros = json!({"prompt": p.prompt, "negative": p.negative, "width": p.width,
                                                    "height": p.height, "steps": p.steps, "seed": p.seed});
                            let _ = fs::write(
                                arquivo.with_extension("json"),
                                serde_json::to_vec_pretty(&json!({"parameters": parametros, "workflow": grafo})).unwrap(),
                            );
                            let caminho = arquivo.display().to_string();
                            let caminho = caminho.strip_prefix(r"\\?\").unwrap_or(&caminho).to_string();
                            return Ok(json!({"ok": true, "file": caminho, "job_id": id, "seed": p.seed,
                                             "model": modelos::nome_curto_imagem(&modelo), "width": p.width, "height": p.height}));
                        }
                        if historico["status"]["completed"] == true {
                            return Err(Erro::Falha("O workflow terminou sem imagem.".into()));
                        }
                    }
                    thread::sleep(Duration::from_millis(250));
                }
                Err(Erro::Falha("A geração excedeu 20 minutos.".into()))
            })();

            // Limpeza: sempre libera a GPU e tenta deixar o Qwen como estava.
            let mut gerado = gerado;
            if let (Err(Erro::Cancelado(_)), Some(pid)) = (&gerado, &prompt_id) {
                st.etapa("cancelling", "Cancelando esta geração...");
                let _ = pedir("POST", &format!("/api/jobs/{pid}/cancel"), Some(json!({})), 3);
            }
            parar_ws.store(true, Ordering::Relaxed);
            st.etapa("stopping_images", "Encerrando o gerador de imagens e liberando a GPU...");
            let mut restaurar = restaurar;
            if let Err(e) = controle::parar_um(c, "imagens", &mut estado, &mut sys) {
                // Não recarrega o Qwen enquanto o ComfyUI pode estar ocupando a VRAM.
                restaurar = false;
                gerado = Err(Erro::Falha(format!("Falha ao encerrar o gerador de imagens: {e}")));
            }
            if restaurar && !global() {
                st.etapa("restoring_llm", "Restaurando o modelo de linguagem...");
                let r = controle::iniciar_servico(c, "inferencia", &mut estado, &mut sys, &|_| {}, &|| {
                    if global() { Err("desligamento".into()) } else { Ok(()) }
                });
                if global() {
                    let _ = controle::parar_um(c, "inferencia", &mut estado, &mut sys);
                    gerado = Err(Erro::Cancelado(Parada::Global));
                } else if let Err(e) = r {
                    aviso = Some(format!("Falha ao restaurar o modelo de linguagem: {e}"));
                }
            }
            gerado
        })
    })();

    let segundos = (inicio.elapsed().as_secs_f64() * 10.0).round() / 10.0;
    let mut final_ = match resultado {
        Ok(mut v) => {
            v["seconds"] = json!(segundos);
            st.dados["completed"] = json!(true);
            st.dados["ok"] = json!(true);
            st.etapa("completed", aviso.as_deref().unwrap_or("Imagem salva. Pronto."));
            v
        }
        Err(Erro::Cancelado(parada)) => {
            let msg = if parada == Parada::Global {
                "Geração cancelada pelo desligamento do ORDIS."
            } else {
                "Geração cancelada."
            };
            st.etapa("cancelled", msg);
            json!({"ok": false, "cancelled": true, "error": msg, "job_id": id})
        }
        Err(Erro::Falha(e)) => {
            st.etapa("failed", &e);
            json!({"ok": false, "error": e, "job_id": id})
        }
    };
    if let Some(a) = aviso {
        final_["warning"] = json!(a);
    }
    final_
}
