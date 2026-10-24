//! Controlador dos serviços do ORDIS: inferência (TabbyAPI) e imagens (ComfyUI).
//!
//! O estado dos processos fica em `runtime\state.json` e a trava de controle em
//! `runtime\control.lock`, compartilhados pelo app e pelo ordis-cli.
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use crate::win;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Serviços controlados; Desligar percorre ao contrário.
pub const SERVICOS: [(&str, u16); 2] = [("inferencia", 5000), ("imagens", 8188)];

pub struct Caminhos {
    pub projeto: PathBuf,
    pub runtime: PathBuf,
}

impl Caminhos {
    pub fn descobrir() -> Result<Self, String> {
        // O executável fica em app/target/<perfil>/; o projeto é o primeiro
        // ancestral com config/paths.json. ORDIS_PROJECT permite sobrescrever.
        let inicio = std::env::var_os("ORDIS_PROJECT")
            .map(PathBuf::from)
            .or_else(|| std::env::current_exe().ok())
            .ok_or("Não foi possível localizar o executável.")?;
        let projeto = inicio
            .ancestors()
            .find(|p| p.join("config/paths.json").is_file())
            .ok_or("config/paths.json não encontrado acima do executável.")?
            .to_path_buf();
        let texto = fs::read_to_string(projeto.join("config/paths.json"))
            .map_err(|e| format!("Falha ao ler paths.json: {e}"))?;
        let v: Value = serde_json::from_str(texto.trim_start_matches('\u{feff}'))
            .map_err(|e| format!("paths.json inválido: {e}"))?;
        // Caminho relativo (o padrão, "runtime") é resolvido a partir do projeto.
        let runtime = projeto.join(v["runtime"].as_str().ok_or("paths.json sem 'runtime'.")?);
        Ok(Caminhos { projeto, runtime })
    }

    fn state(&self) -> PathBuf {
        self.runtime.join("state.json")
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Processo {
    pub pid: u32,
    pub created: f64,
    pub cmdline: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_file: Option<String>,
}

pub(crate) type Estado = BTreeMap<String, Processo>;

pub(crate) fn ler_estado(c: &Caminhos) -> Estado {
    fs::read_to_string(c.state())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn salvar(c: &Caminhos, estado: &Estado) -> Result<(), String> {
    let tmp = c.state().with_extension("tmp");
    let json = serde_json::to_string_pretty(estado).map_err(|e| e.to_string())?;
    fs::write(&tmp, json).map_err(|e| format!("Falha ao gravar estado: {e}"))?;
    fs::rename(&tmp, c.state()).map_err(|e| format!("Falha ao gravar estado: {e}"))
}

fn linha_de_comando(sys: &mut System, pid: u32) -> Vec<String> {
    let pid = Pid::from_u32(pid);
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
    );
    sys.process(pid)
        .map(|p| p.cmd().iter().map(|s| s.to_string_lossy().into_owned()).collect())
        .unwrap_or_default()
}

/// Confirma que o PID ainda é o processo registrado (mesmo início e mesma linha de comando).
pub(crate) fn rastreado(sys: &mut System, info: &Processo) -> bool {
    match win::creation_time(info.pid) {
        Some(t) if (t - info.created).abs() <= 0.1 => {}
        _ => return false,
    }
    if !win::is_running(info.pid) {
        return false;
    }
    let cmd = linha_de_comando(sys, info.pid);
    // Sem acesso à linha de comando, o horário de criação já identifica o processo.
    cmd.is_empty() || cmd == info.cmdline
}

pub(crate) fn ocupada(porta: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], porta));
    TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok()
}

pub fn chave_api(c: &Caminhos) -> Option<String> {
    // api_tokens.yml do Tabby: `api_key: valor` (ou lista de um item).
    let texto = fs::read_to_string(c.runtime.join("tabbyAPI/api_tokens.yml")).ok()?;
    let mut linhas = texto.lines();
    while let Some(l) = linhas.next() {
        if let Some(resto) = l.strip_prefix("api_key:") {
            let resto = resto.trim();
            let valor = if resto.is_empty() {
                linhas.next()?.trim().trim_start_matches('-').trim()
            } else {
                resto
            };
            return Some(valor.trim_matches(['"', '\'']).to_string());
        }
    }
    None
}

fn buscar(url: &str, chave: Option<&str>) -> Option<Value> {
    let mut req = ureq::get(url).timeout(Duration::from_secs(3));
    if let Some(k) = chave {
        req = req.set("Authorization", &format!("Bearer {k}"));
    }
    req.call().ok()?.into_json().ok()
}

pub(crate) fn pronto(c: &Caminhos, nome: &str) -> bool {
    match nome {
        "imagens" => buscar("http://127.0.0.1:8188/system_stats", None)
            .is_some_and(|v| v.get("system").is_some()),
        _ => {
            let Some(chave) = chave_api(c) else { return false };
            buscar("http://127.0.0.1:5000/v1/models", Some(&chave)).is_some_and(|v| {
                v["data"].as_array().is_some_and(|d| d.iter().any(|m| m["id"] == crate::modelos::conversa(c).modelo.as_str()))
            })
        }
    }
}

pub fn vram() -> Option<String> {
    let out = Command::new("nvidia-smi")
        .args(["--query-gpu=memory.used,memory.total", "--format=csv,noheader"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[derive(Serialize)]
pub struct EstadoServico {
    pub nome: &'static str,
    pub porta: u16,
    /// "pronto", "iniciando" ou "desligado".
    pub situacao: &'static str,
}

#[derive(Serialize)]
pub struct Painel {
    pub servicos: Vec<EstadoServico>,
    pub vram: Option<String>,
    pub tokens: Option<Value>,
    /// Nomes curtos dos modelos configurados, para a interface.
    pub modelos: Value,
}

pub fn painel(c: &Caminhos) -> Painel {
    let estado = ler_estado(c);
    let mut sys = System::new();
    let servicos = SERVICOS
        .iter()
        .map(|&(nome, porta)| {
            let ativo = estado.get(nome).is_some_and(|i| rastreado(&mut sys, i));
            let situacao = match (ativo, ativo && pronto(c, nome)) {
                (_, true) => "pronto",
                (true, false) => "iniciando",
                _ => "desligado",
            };
            EstadoServico { nome, porta, situacao }
        })
        .collect();
    let tokens = fs::read_to_string(c.runtime.join("token-metrics.json"))
        .ok()
        .and_then(|t| serde_json::from_str(t.trim()).ok());
    Painel { servicos, vram: vram(), tokens, modelos: crate::modelos::nomes(c) }
}

/// Trava exclusiva de controle (`runtime\control.lock`): só um controle por vez.
pub(crate) fn com_trava<T, E: From<String>>(
    c: &Caminhos,
    esperar: bool,
    f: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    fs::create_dir_all(&c.runtime).map_err(|e| E::from(e.to_string()))?;
    let mut arq = OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(c.runtime.join("control.lock"))
        .map_err(|e| E::from(format!("Falha ao abrir control.lock: {e}")))?;
    let mut b = [0u8; 1];
    if arq.read(&mut b).unwrap_or(0) == 0 {
        let _ = arq.write_all(b"0");
    }
    let limite = Instant::now() + Duration::from_secs(90);
    let _trava = loop {
        if let Some(t) = win::ByteLock::try_lock(&arq) {
            break t;
        }
        if !esperar || Instant::now() > limite {
            return Err(E::from("Outro controle do ORDIS está em andamento. Aguarde.".to_string()));
        }
        sleep(Duration::from_secs(1));
    };
    f()
}

fn descendentes(sys: &mut System, raiz: u32) -> Vec<u32> {
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let mut encontrados = vec![];
    let mut fila = vec![Pid::from_u32(raiz)];
    while let Some(pai) = fila.pop() {
        for (pid, p) in sys.processes() {
            if p.parent() == Some(pai) {
                encontrados.push(pid.as_u32());
                fila.push(*pid);
            }
        }
    }
    encontrados
}

pub(crate) fn parar_um(c: &Caminhos, nome: &str, estado: &mut Estado, sys: &mut System) -> Result<(), String> {
    let Some(info) = estado.get(nome).cloned() else { return Ok(()) };
    if rastreado(sys, &info) {
        if let Some(stop) = &info.stop_file {
            let _ = File::create(stop);
            if win::wait(info.pid, Duration::from_secs(20)) {
                estado.remove(nome);
                return salvar(c, estado);
            }
        }
        // Só o processo registrado e seus próprios descendentes são encerrados.
        let filhos = descendentes(sys, info.pid);
        for &pid in filhos.iter().rev() {
            win::terminate(pid);
        }
        win::terminate(info.pid);
        let todos: Vec<u32> = filhos.iter().copied().chain([info.pid]).collect();
        let limite = Instant::now() + Duration::from_secs(20);
        while todos.iter().any(|&p| win::is_running(p)) {
            if Instant::now() > limite {
                return Err(format!("Não foi possível encerrar {nome}; estado preservado."));
            }
            sleep(Duration::from_millis(250));
        }
    }
    estado.remove(nome);
    salvar(c, estado)
}

pub fn sufixo_unico() -> String {
    let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    format!("{:016x}", (n as u64) ^ ((std::process::id() as u64) << 40))
}

/// Inicia a inferência (TabbyAPI) ou o gerador de imagens (ComfyUI) e espera
/// ficar pronto. `cancelado` é consultado durante a espera.
pub(crate) fn iniciar_servico(
    c: &Caminhos,
    nome: &str,
    estado: &mut Estado,
    sys: &mut System,
    progresso: &dyn Fn(&str),
    cancelado: &dyn Fn() -> Result<(), String>,
) -> Result<(), String> {
    let porta = SERVICOS.iter().find(|(n, _)| *n == nome).map(|(_, p)| *p).ok_or("Serviço desconhecido.")?;
    if let Some(info) = estado.get(nome) {
        if rastreado(sys, info) {
            if pronto(c, nome) {
                return Ok(());
            }
            return Err(format!("{nome} já está em execução, mas sem resposta. Use Desligar."));
        }
    }
    if ocupada(porta) {
        return Err(format!("Porta {porta} ocupada por outro processo. Nenhum processo externo foi encerrado."));
    }
    if nome == "inferencia" && ocupada(5001) {
        // O Tabby pode trocar para esta porta; melhor falhar do que mudar em silêncio.
        return Err("Porta 5001 ocupada; verifique antes de iniciar a inferência.".into());
    }
    cancelado()?;
    let r = &c.runtime;
    let mut cmd;
    let mut stop_file = None;
    if nome == "inferencia" {
        let cwd = r.join("tabbyAPI");
        cmd = Command::new(cwd.join("venv/Scripts/python.exe"));
        cmd.arg(c.projeto.join("scripts/serve_inference.py")).current_dir(&cwd);
        // serve_inference.py encerra o Tabby de forma ordenada quando este arquivo aparece.
        let f = r.join(format!("{nome}-{}.stop", sufixo_unico()));
        cmd.env("ORDIS_STOP_FILE", &f);
        stop_file = Some(f.display().to_string());
    } else {
        let cwd = r.join("ComfyUI");
        let saida = crate::imagens::pasta_saida(c);
        fs::create_dir_all(&saida).map_err(|e| e.to_string())?;
        cmd = Command::new(r.join("images-venv/Scripts/python.exe"));
        cmd.arg(cwd.join("main.py"))
            .args(["--listen", "127.0.0.1", "--port", "8188", "--output-directory"])
            .arg(&saida)
            .args(["--disable-all-custom-nodes", "--disable-api-nodes", "--disable-auto-launch"])
            .current_dir(&cwd);
    }
    fs::create_dir_all(r.join("logs")).map_err(|e| e.to_string())?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(r.join(format!("logs/{nome}.log")))
        .map_err(|e| format!("Falha ao abrir o log: {e}"))?;
    let erro = log.try_clone().map_err(|e| e.to_string())?;
    win::nao_herdar_console();
    let filho = cmd
        .env("XDG_CACHE_HOME", r.join("cache"))
        .env("TRITON_CACHE_DIR", r.join("cache/triton"))
        .env("TORCHINDUCTOR_CACHE_DIR", r.join("cache/torchinductor"))
        .env("PYTHONUTF8", "1")
        .env("PYTHONUNBUFFERED", "1")
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(erro)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("Falha ao iniciar {nome}: {e}"))?;
    let pid = filho.id();
    let info = Processo {
        pid,
        created: win::creation_time(pid).ok_or(format!("{nome} encerrou ao iniciar."))?,
        cmdline: linha_de_comando(sys, pid),
        stop_file,
    };
    estado.insert(nome.into(), info);
    salvar(c, estado)?;
    progresso(if nome == "inferencia" { "Carregando o modelo na GPU..." } else { "Iniciando o gerador de imagens..." });
    let limite = Instant::now() + Duration::from_secs(300);
    while Instant::now() < limite {
        cancelado()?;
        if !win::is_running(pid) {
            return Err(format!("{nome} encerrou. Consulte runtime\\logs\\{nome}.log."));
        }
        if pronto(c, nome) {
            progresso(if nome == "inferencia" { "Inferência pronta." } else { "Gerador de imagens pronto." });
            return Ok(());
        }
        sleep(Duration::from_secs(1));
    }
    Err(format!("{nome} excedeu o tempo de inicialização."))
}

pub fn iniciar(c: &Caminhos, progresso: &dyn Fn(&str)) -> Result<(), String> {
    com_trava(c, false, || {
        let mut estado = ler_estado(c);
        let mut sys = System::new();
        let antes: Vec<String> = estado
            .iter()
            .filter(|(_, i)| rastreado(&mut sys, i))
            .map(|(n, _)| n.clone())
            .collect();
        let resultado = (|| {
            // Imagens e Qwen não cabem juntos na GPU.
            if estado.contains_key("imagens") {
                progresso("Liberando a GPU do gerador de imagens...");
                parar_um(c, "imagens", &mut estado, &mut sys)?;
            }
            iniciar_servico(c, "inferencia", &mut estado, &mut sys, progresso, &|| Ok(()))
        })();
        if resultado.is_err() {
            for (nome, _) in SERVICOS.iter().rev() {
                if !antes.iter().any(|a| a == nome) {
                    let _ = parar_um(c, nome, &mut estado, &mut sys);
                }
            }
        }
        resultado
    })
}

/// Marcador de desligamento: uma geração de imagem em andamento o observa e se cancela.
pub(crate) fn marcador_desligamento(c: &Caminhos) -> PathBuf {
    c.runtime.join("images.cancel")
}

pub fn parar(c: &Caminhos, progresso: &dyn Fn(&str)) -> Result<(), String> {
    // Pede o cancelamento de uma geração de imagem ou avaliação antes de disputar a trava.
    let _ = fs::write(marcador_desligamento(c), sufixo_unico());
    com_trava(c, true, || {
        let mut estado = ler_estado(c);
        let mut sys = System::new();
        for (nome, _) in SERVICOS.iter().rev() {
            if estado.contains_key(*nome) {
                progresso(&format!("Encerrando {nome}..."));
                parar_um(c, nome, &mut estado, &mut sys)?;
            }
        }
        progresso("Serviços encerrados.");
        Ok(())
    })
}

pub fn pasta_logs(c: &Caminhos) -> PathBuf {
    c.runtime.join("logs")
}

pub fn inferencia_pronta(c: &Caminhos) -> bool {
    pronto(c, "inferencia")
}
