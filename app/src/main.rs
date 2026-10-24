#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agente;
mod controle;
mod ferramentas;
mod imagens;
mod modelos;
mod proxmox;
mod win;

use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use controle::{Caminhos, Painel};

struct App {
    caminhos: Caminhos,
    /// Sinal de cancelamento da resposta em andamento; o agente atende uma por vez.
    resposta: Mutex<Option<Arc<AtomicBool>>>,
    /// Sinal de cancelamento da geração pedida pela aba Imagens.
    imagem: Mutex<Option<Arc<AtomicBool>>>,
}

type Ctx = Arc<App>;

fn emissor(app: AppHandle) -> impl Fn(&str) {
    move |msg: &str| {
        let _ = app.emit("progresso", msg.to_string());
    }
}

struct EventosAgente {
    app: AppHandle,
    sessao: String,
}

impl agente::Saida for EventosAgente {
    fn evento(&self, tipo: &str, dados: Value) {
        let _ = self.app.emit("agente", json!({"sessao": self.sessao, "tipo": tipo, "dados": dados}));
    }
}

async fn em_segundo_plano<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn painel(c: State<'_, Ctx>) -> Result<Painel, String> {
    let c = c.inner().clone();
    em_segundo_plano(move || Ok(controle::painel(&c.caminhos))).await
}

#[tauri::command]
async fn iniciar(app: AppHandle, c: State<'_, Ctx>) -> Result<(), String> {
    let c = c.inner().clone();
    em_segundo_plano(move || controle::iniciar(&c.caminhos, &emissor(app))).await
}

#[tauri::command]
async fn parar(app: AppHandle, c: State<'_, Ctx>) -> Result<(), String> {
    let c = c.inner().clone();
    if let Some(r) = c.resposta.lock().unwrap().as_ref() {
        r.store(true, Ordering::Relaxed);
    }
    em_segundo_plano(move || controle::parar(&c.caminhos, &emissor(app))).await
}

#[tauri::command]
fn abrir_logs(c: State<'_, Ctx>) -> Result<(), String> {
    Command::new("explorer.exe")
        .arg(controle::pasta_logs(&c.caminhos))
        .creation_flags(0x0800_0000)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn sessoes(c: State<'_, Ctx>) -> Vec<agente::Resumo> {
    agente::listar(&c.caminhos)
}

#[tauri::command]
fn sessao(c: State<'_, Ctx>, id: String) -> Result<agente::Sessao, String> {
    agente::abrir(&c.caminhos, &id)
}

#[tauri::command]
fn nova_sessao(c: State<'_, Ctx>) -> Result<agente::Sessao, String> {
    agente::nova(&c.caminhos)
}

#[tauri::command]
fn apagar_sessao(c: State<'_, Ctx>, id: String) -> Result<(), String> {
    agente::apagar(&c.caminhos, &id)
}

#[tauri::command]
fn renomear_sessao(c: State<'_, Ctx>, id: String, titulo: String) -> Result<(), String> {
    agente::renomear(&c.caminhos, &id, &titulo)
}

#[tauri::command]
async fn enviar(
    app: AppHandle,
    c: State<'_, Ctx>,
    id: String,
    texto: String,
    anexos: Vec<String>,
) -> Result<(), String> {
    responder(app, c.inner().clone(), id.clone(), move |c, cancelar, saida| {
        agente::responder(c, &id, texto, anexos, cancelar, saida)
    })
    .await
}

#[tauri::command]
async fn refazer(app: AppHandle, c: State<'_, Ctx>, id: String) -> Result<(), String> {
    responder(app, c.inner().clone(), id.clone(), move |c, cancelar, saida| {
        agente::refazer(c, &id, cancelar, saida)
    })
    .await
}

/// Roda uma resposta do agente por vez, interrompível pelo `interromper`.
async fn responder(
    app: AppHandle,
    c: Ctx,
    id: String,
    f: impl FnOnce(&Caminhos, &AtomicBool, &EventosAgente) -> Result<(), String> + Send + 'static,
) -> Result<(), String> {
    let cancelar = Arc::new(AtomicBool::new(false));
    {
        let mut atual = c.resposta.lock().unwrap();
        if atual.is_some() {
            return Err("O agente ainda está respondendo. Aguarde ou interrompa.".into());
        }
        *atual = Some(cancelar.clone());
    }
    let saida = EventosAgente { app, sessao: id };
    let c2 = c.clone();
    let resultado = em_segundo_plano(move || f(&c2.caminhos, &cancelar, &saida)).await;
    *c.resposta.lock().unwrap() = None;
    resultado
}

#[tauri::command]
fn interromper(c: State<'_, Ctx>) {
    if let Some(r) = c.resposta.lock().unwrap().as_ref() {
        r.store(true, Ordering::Relaxed);
    }
}

/// Abre links das respostas no navegador padrão, fora da janela do ORDIS.
#[tauri::command]
fn abrir_url(url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Somente links http ou https podem ser abertos.".into());
    }
    Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", &url])
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Imagem do workspace como data URL, para exibir no chat.
#[tauri::command]
async fn imagem(c: State<'_, Ctx>, caminho: String) -> Result<String, String> {
    let c = c.inner().clone();
    em_segundo_plano(move || {
        let alvo = ferramentas::dentro_do_workspace(&c.caminhos, &caminho)?;
        agente::data_url(&alvo)
    })
    .await
}

#[tauri::command]
async fn gerar_imagem(app: AppHandle, c: State<'_, Ctx>, parametros: Value) -> Result<Value, String> {
    let c = c.inner().clone();
    let cancelar = Arc::new(AtomicBool::new(false));
    {
        let mut atual = c.imagem.lock().unwrap();
        if atual.is_some() {
            return Err("Já existe uma geração em andamento.".into());
        }
        *atual = Some(cancelar.clone());
    }
    let c2 = c.clone();
    let resultado = em_segundo_plano(move || {
        Ok(imagens::gerar(&c2.caminhos, &parametros, &cancelar, &|s| {
            let _ = app.emit("imagem", s.clone());
        }))
    })
    .await;
    *c.imagem.lock().unwrap() = None;
    resultado
}

#[tauri::command]
fn cancelar_imagem(c: State<'_, Ctx>) {
    if let Some(r) = c.imagem.lock().unwrap().as_ref() {
        r.store(true, Ordering::Relaxed);
    }
}

/// PNGs gerados, do mais recente para o mais antigo, com os parâmetros
/// gravados no .json ao lado (quando existe).
#[tauri::command]
fn imagens_recentes(c: State<'_, Ctx>) -> Vec<Value> {
    let pasta = ferramentas::workspace(&c.caminhos).join("imagens");
    let mut lista: Vec<(std::time::SystemTime, String)> = std::fs::read_dir(pasta)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("png")))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path().display().to_string())))
        .collect();
    lista.sort_by(|a, b| b.0.cmp(&a.0));
    lista
        .into_iter()
        .take(60)
        .map(|(_, p)| {
            let parametros = std::fs::read_to_string(std::path::Path::new(&p).with_extension("json"))
                .ok()
                .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                .map(|v| v["parameters"].clone())
                .unwrap_or(Value::Null);
            serde_json::json!({"file": p, "parameters": parametros})
        })
        .collect()
}

/// Abre um arquivo ou pasta do workspace no Windows.
#[tauri::command]
fn abrir_no_windows(c: State<'_, Ctx>, caminho: String) -> Result<(), String> {
    let alvo = ferramentas::dentro_do_workspace(&c.caminhos, &caminho)?;
    Command::new("explorer.exe").arg(alvo).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command]
async fn modelos_opcoes(c: State<'_, Ctx>) -> Result<Value, String> {
    let c = c.inner().clone();
    em_segundo_plano(move || Ok(modelos::opcoes(&c.caminhos))).await
}

/// Grava a configuração dos modelos. Se o modelo de conversa mudou e o Tabby
/// estava ligado, reinicia para valer na hora; desligado, vale na próxima carga.
#[tauri::command]
async fn modelos_salvar(
    app: AppHandle,
    c: State<'_, Ctx>,
    conversa: modelos::Conversa,
    imagem: modelos::Imagem,
) -> Result<Value, String> {
    let c = c.inner().clone();
    if c.resposta.lock().unwrap().is_some() {
        return Err("O agente está respondendo. Aguarde ou interrompa antes de trocar o modelo.".into());
    }
    if c.imagem.lock().unwrap().is_some() {
        return Err("Há uma imagem sendo gerada. Aguarde ou cancele antes de trocar o modelo.".into());
    }
    em_segundo_plano(move || {
        let ligado = controle::painel(&c.caminhos).servicos.iter().any(|s| s.nome == "inferencia" && s.situacao != "desligado");
        let mudou = modelos::salvar(&c.caminhos, &conversa, &imagem)?;
        let reiniciar = mudou && ligado;
        if reiniciar {
            let p = emissor(app);
            p("Trocando o modelo de conversa…");
            controle::parar(&c.caminhos, &p)?;
            controle::iniciar(&c.caminhos, &p)?;
        }
        Ok(json!({"mudou_conversa": mudou, "reiniciado": reiniciar}))
    })
    .await
}

#[tauri::command]
async fn modelos_testar_conversa(app: AppHandle, c: State<'_, Ctx>) -> Result<Value, String> {
    let c = c.inner().clone();
    if c.resposta.lock().unwrap().is_some() || c.imagem.lock().unwrap().is_some() {
        return Err("Aguarde a resposta ou a imagem em andamento terminar.".into());
    }
    em_segundo_plano(move || Ok(modelos::testar_conversa(&c.caminhos, &emissor(app)))).await
}

#[tauri::command]
fn modelos_importar_fluxo(c: State<'_, Ctx>, nome: String, conteudo: String) -> Result<String, String> {
    modelos::importar_fluxo(&c.caminhos, &nome, &conteudo)
}

/// Abre no Explorer a pasta de modelos de conversa, de imagem ou de fluxos.
#[tauri::command]
fn modelos_abrir_pasta(c: State<'_, Ctx>, qual: String) -> Result<(), String> {
    let v = modelos::opcoes(&c.caminhos);
    let pasta = v["pastas"][qual.as_str()].as_str().ok_or("Pasta desconhecida.")?.to_string();
    std::fs::create_dir_all(&pasta).map_err(|e| e.to_string())?;
    Command::new("explorer.exe").arg(pasta).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command]
fn pessoal(c: State<'_, Ctx>) -> Value {
    agente::pessoal(&c.caminhos)
}

#[tauri::command]
fn pessoal_salvar(c: State<'_, Ctx>, tratamento: String, sobre: String) -> Result<(), String> {
    agente::salvar_pessoal(&c.caminhos, &tratamento, &sobre)
}

#[tauri::command]
fn proxmox_estado(c: State<'_, Ctx>) -> proxmox::Conexao {
    proxmox::estado(&c.caminhos)
}

#[tauri::command]
async fn proxmox_salvar(
    c: State<'_, Ctx>,
    url: String,
    token_id: String,
    segredo: String,
    ca_pem: Option<String>,
) -> Result<Value, String> {
    let c = c.inner().clone();
    em_segundo_plano(move || proxmox::salvar(&c.caminhos, &url, &token_id, &segredo, ca_pem)).await
}

#[tauri::command]
async fn proxmox_testar(c: State<'_, Ctx>) -> Result<Value, String> {
    let c = c.inner().clone();
    em_segundo_plano(move || proxmox::testar(&c.caminhos)).await
}

fn main() {
    let caminhos = match Caminhos::descobrir() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ORDIS: {e}");
            std::process::exit(1);
        }
    };
    tauri::Builder::default()
        .manage::<Ctx>(Arc::new(App { caminhos, resposta: Mutex::new(None), imagem: Mutex::new(None) }))
        .setup(|app| {
            // A janela nasce oculta e a interface a mostra quando termina de
            // carregar. Se passar de 500 ms (PC lento, abertura a frio), ela
            // aparece mesmo assim, com o anel de carregamento do index.html.
            if let Some(janela) = app.get_webview_window("main") {
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    let _ = janela.show();
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            painel,
            iniciar,
            parar,
            abrir_logs,
            sessoes,
            sessao,
            nova_sessao,
            apagar_sessao,
            renomear_sessao,
            refazer,
            enviar,
            interromper,
            abrir_url,
            imagem,
            gerar_imagem,
            cancelar_imagem,
            imagens_recentes,
            abrir_no_windows,
            pessoal,
            pessoal_salvar,
            proxmox_estado,
            proxmox_salvar,
            proxmox_testar,
            modelos_opcoes,
            modelos_salvar,
            modelos_testar_conversa,
            modelos_importar_fluxo,
            modelos_abrir_pasta
        ])
        .run(tauri::generate_context!())
        .expect("falha ao iniciar a janela do ORDIS");
}
