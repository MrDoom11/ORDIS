//! Conexão e consultas somente leitura ao Proxmox. A configuração fica em
//! runtime\proxmox: connection.json, token.dpapi (DPAPI) e a CA do servidor.
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use windows_sys::Win32::Foundation::LocalFree;
use rustls::pki_types::{pem::PemObject, CertificateDer};
use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB};

use crate::controle::Caminhos;

fn pasta(c: &Caminhos) -> PathBuf {
    c.runtime.join("proxmox")
}

#[derive(Serialize)]
pub struct Conexao {
    pub configurado: bool,
    pub url: String,
    pub token_id: String,
    pub ca: bool,
}

pub fn estado(c: &Caminhos) -> Conexao {
    let p = pasta(c);
    let cfg: Value = fs::read_to_string(p.join("connection.json"))
        .ok()
        .and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())
        .unwrap_or(Value::Null);
    Conexao {
        configurado: cfg.is_object() && p.join("token.dpapi").is_file(),
        url: cfg["url"].as_str().unwrap_or("").to_string(),
        token_id: cfg["token_id"].as_str().unwrap_or("").to_string(),
        ca: cfg["ca_file"].as_str().is_some_and(|f| PathBuf::from(f).is_file()),
    }
}

/// Criptografa com DPAPI para o usuário Windows atual, como o configurador antigo.
fn proteger(segredo: &str) -> Result<Vec<u8>, String> {
    let mut dados = segredo.as_bytes().to_vec();
    let entrada = CRYPT_INTEGER_BLOB { cbData: dados.len() as u32, pbData: dados.as_mut_ptr() };
    let mut saida = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let ok = unsafe {
        CryptProtectData(&entrada, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), 0, &mut saida)
    };
    dados.fill(0);
    if ok == 0 {
        return Err("Não foi possível proteger o token com o usuário Windows.".into());
    }
    let protegido = unsafe { std::slice::from_raw_parts(saida.pbData, saida.cbData as usize).to_vec() };
    unsafe { LocalFree(saida.pbData as _) };
    Ok(protegido)
}

fn token_id_valido(id: &str) -> bool {
    // usuario@realm!nome, sem espaços nem '='.
    let Some((usuario, nome)) = id.split_once('!') else { return false };
    let Some((u, realm)) = usuario.split_once('@') else { return false };
    let limpo = |s: &str| !s.is_empty() && !s.chars().any(|ch| ch.is_whitespace() || ch == '=' || ch == '!');
    limpo(u) && limpo(realm) && limpo(nome) && !u.contains('@')
}

/// Grava a conexão e testa com a consulta `nos`. O segredo vazio mantém o atual.
pub fn salvar(
    c: &Caminhos,
    url: &str,
    token_id: &str,
    segredo: &str,
    ca_pem: Option<String>,
) -> Result<Value, String> {
    let url = url.trim().trim_end_matches('/');
    let host = url.strip_prefix("https://").ok_or("A URL deve começar com https://.")?;
    if host.is_empty() || host.contains(['/', '?', '#', '@']) {
        return Err("Use só o endereço do servidor, como https://proxmox.local:8006.".into());
    }
    let token_id = token_id.trim();
    if !token_id_valido(token_id) {
        return Err("ID do token inválido. Formato: usuario@pve!nome.".into());
    }
    let p = pasta(c);
    fs::create_dir_all(&p).map_err(|e| e.to_string())?;
    let ca = p.join("pve-root-ca.pem");
    if let Some(pem) = ca_pem {
        if !pem.contains("-----BEGIN CERTIFICATE-----") {
            return Err("O arquivo da CA não parece um certificado PEM.".into());
        }
        fs::write(&ca, pem).map_err(|e| format!("Falha ao salvar a CA: {e}"))?;
    }
    if !ca.is_file() {
        return Err("Selecione o pve-root-ca.pem exportado do seu Proxmox.".into());
    }
    let segredo = segredo.trim();
    if segredo.is_empty() {
        if !p.join("token.dpapi").is_file() {
            return Err("Informe o segredo do token.".into());
        }
    } else {
        fs::write(p.join("token.dpapi"), proteger(segredo)?).map_err(|e| format!("Falha ao salvar o token: {e}"))?;
    }
    let cfg = json!({"url": url, "token_id": token_id, "ca_file": ca.display().to_string()});
    fs::write(p.join("connection.json"), serde_json::to_vec_pretty(&cfg).unwrap())
        .map_err(|e| format!("Falha ao salvar a conexão: {e}"))?;
    testar(c)
}

pub fn testar(c: &Caminhos) -> Result<Value, String> {
    consultar(c, "nos", None)
}

/// Decifra o token gravado com DPAPI para o usuário Windows atual.
fn decifrar(dados: &[u8]) -> Result<String, String> {
    let mut entrada_buf = dados.to_vec();
    let entrada = CRYPT_INTEGER_BLOB { cbData: entrada_buf.len() as u32, pbData: entrada_buf.as_mut_ptr() };
    let mut saida = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let ok = unsafe {
        CryptUnprotectData(
            &entrada,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            &mut saida,
        )
    };
    if ok == 0 {
        return Err("Não foi possível abrir a credencial protegida para este usuário Windows.".into());
    }
    let bytes = unsafe { std::slice::from_raw_parts_mut(saida.pbData, saida.cbData as usize) };
    let texto = String::from_utf8(bytes.to_vec()).map_err(|_| "Token corrompido.".to_string());
    bytes.fill(0);
    unsafe { LocalFree(saida.pbData as _) };
    texto
}

struct Cliente {
    agente: ureq::Agent,
    url: String,
    autorizacao: String,
    chamadas: std::cell::Cell<u32>,
}

impl Cliente {
    fn abrir(c: &Caminhos) -> Result<(Self, String), String> {
        let p = pasta(c);
        if !p.join("connection.json").is_file() || !p.join("token.dpapi").is_file() {
            return Err("Proxmox ainda não configurado. Configure a conexão em Configurações, no app ORDIS.".into());
        }
        let cfg: Value = serde_json::from_str(
            fs::read_to_string(p.join("connection.json")).map_err(|e| e.to_string())?.trim_start_matches('\u{feff}'),
        )
        .map_err(|e| format!("connection.json inválido: {e}"))?;
        let url = cfg["url"].as_str().unwrap_or("").trim_end_matches('/').to_string();
        let host = url.strip_prefix("https://").unwrap_or("");
        if host.is_empty() || host.contains(['/', '?', '#', '@']) {
            return Err("URL HTTPS de origem inválida.".into());
        }
        let token_id = cfg["token_id"].as_str().ok_or("connection.json sem token_id.")?;
        let token = decifrar(&fs::read(p.join("token.dpapi")).map_err(|e| e.to_string())?)?;
        // Confia apenas na CA do próprio Proxmox, como o configurador definiu.
        let ca = cfg["ca_file"].as_str().map(PathBuf::from).unwrap_or_else(|| p.join("pve-root-ca.pem"));
        let mut raizes = rustls::RootCertStore::empty();
        for cert in CertificateDer::pem_file_iter(&ca).map_err(|_| "Certificado da CA não encontrado.".to_string())? {
            raizes
                .add(cert.map_err(|_| "Certificado da CA inválido.".to_string())?)
                .map_err(|_| "Certificado da CA inválido.".to_string())?;
        }
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_root_certificates(raizes)
            .with_no_client_auth();
        let agente = ureq::AgentBuilder::new()
            .tls_config(Arc::new(tls))
            .redirects(0)
            .timeout(Duration::from_secs(15))
            .build();
        let cliente = Cliente {
            agente,
            url: url.clone(),
            autorizacao: format!("PVEAPIToken={token_id}={token}"),
            chamadas: std::cell::Cell::new(0),
        };
        Ok((cliente, url))
    }

    fn dados(&self, caminho: &str) -> Result<Value, String> {
        self.chamadas.set(self.chamadas.get() + 1);
        if self.chamadas.get() > 50 {
            return Err("Limite de consultas atingido; filtre pelo nó.".into());
        }
        let r = self
            .agente
            .get(&format!("{}/api2/json{caminho}", self.url))
            .set("Authorization", &self.autorizacao)
            .set("Accept", "application/json")
            .call()
            .map_err(|e| match e {
                ureq::Error::Status(code, _) => {
                    format!("Proxmox HTTP {code}. Verifique token e permissões PVEAuditor do usuário e do token.")
                }
                _ => "Falha de HTTPS/rede ao consultar o Proxmox. Confira URL, certificado CA e validade do certificado."
                    .into(),
            })?;
        if r.status() >= 300 {
            return Err("Redirecionamento recusado; confira a URL do Proxmox.".into());
        }
        let mut corpo = Vec::new();
        r.into_reader().take(2_000_001).read_to_end(&mut corpo).map_err(|e| e.to_string())?;
        if corpo.len() > 2_000_000 {
            return Err("Resposta grande demais; restrinja a consulta.".into());
        }
        let v: Value = serde_json::from_slice(&corpo).map_err(|_| "Resposta inválida do Proxmox.".to_string())?;
        Ok(v["data"].clone())
    }
}

fn nome_valido(v: &str) -> bool {
    let mut cs = v.chars();
    v.len() <= 63
        && cs.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && cs.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn caminho(consulta: &str, no: Option<&str>) -> Result<String, String> {
    if let Some(n) = no {
        if !nome_valido(n) {
            return Err("Nome de nó inválido.".into());
        }
    }
    Ok(match (consulta, no) {
        ("armazenamento", Some(n)) => format!("/nodes/{n}/storage"),
        ("estado_no", Some(n)) => format!("/nodes/{n}/status"),
        ("versao", _) => "/version".into(),
        ("nos", _) => "/nodes".into(),
        ("resumo", _) => "/cluster/resources".into(),
        ("vms", _) => "/cluster/resources?type=vm".into(),
        ("armazenamento", None) => "/cluster/resources?type=storage".into(),
        _ => return Err("Consulta não permitida ou nó obrigatório ausente.".into()),
    })
}

const CAMPOS: [&str; 31] = [
    "id", "node", "type", "status", "name", "vmid", "template", "cpu", "maxcpu", "mem", "maxmem", "disk", "maxdisk",
    "uptime", "storage", "content", "enabled", "active", "shared", "total", "used", "avail", "version", "release",
    "repoid", "pveversion", "loadavg", "memory", "rootfs", "swap", "cpuinfo",
];

/// Mantém só os campos conhecidos e limita listas a 200 itens.
fn sanitizar(v: Value) -> Value {
    match v {
        Value::Array(lista) => {
            let total = lista.len();
            json!({"items": lista.into_iter().take(200).map(sanitizar).collect::<Vec<_>>(),
                   "total_items": total, "truncated": total > 200})
        }
        Value::Object(mapa) => {
            let mut out: serde_json::Map<String, Value> =
                mapa.into_iter().filter(|(k, _)| CAMPOS.contains(&k.as_str())).collect();
            let tipo = out.get("type").and_then(Value::as_str).unwrap_or("");
            if matches!(tipo, "node" | "storage") {
                if let (Some(max), Some(disk)) =
                    (out.get("maxdisk").and_then(Value::as_f64), out.get("disk").and_then(Value::as_f64))
                {
                    out.insert("free_bytes".into(), json!((max - disk).max(0.0) as u64));
                }
            }
            Value::Object(out)
        }
        outro => outro,
    }
}

fn data_utc(segundos: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(segundos, 0).map(|d| d.to_rfc3339())
}

/// Backups individuais por storage, ou discos por slot de cada VM/CT.
fn detalhes(cli: &Cliente, consulta: &str, no: Option<&str>) -> Result<Value, String> {
    let mut itens: Vec<Value> = vec![];
    let mut erros: Vec<Value> = vec![];
    let mut verificados: Vec<Value> = vec![];
    let erro = |escopo: &str, e: &str, erros: &mut Vec<Value>| {
        let tipo = e.split('.').next().unwrap_or(e).to_string();
        erros.push(json!({"scope": escopo, "error": tipo}));
    };
    let truncado;
    let nota;
    if consulta == "backups" {
        let nos: Vec<String> = match no {
            Some(n) => vec![n.to_string()],
            None => cli.dados("/nodes")?
                .as_array()
                .map(|a| a.iter().filter_map(|n| n["node"].as_str().map(String::from)).collect())
                .unwrap_or_default(),
        };
        truncado = nos.len() > 20;
        for host in nos.iter().take(20) {
            if !nome_valido(host) {
                return Err("Identificador inválido retornado pela API.".into());
            }
            let storages = match cli.dados(&format!("/nodes/{host}/storage")) {
                Ok(s) => s,
                Err(e) => {
                    erro(host, &e, &mut erros);
                    continue;
                }
            };
            for s in storages.as_array().into_iter().flatten() {
                if !s["content"].as_str().unwrap_or("").split(',').any(|x| x == "backup") {
                    continue;
                }
                let sid = s["storage"].as_str().unwrap_or("");
                if !nome_valido(sid) {
                    return Err("Identificador inválido retornado pela API.".into());
                }
                match cli.dados(&format!("/nodes/{host}/storage/{sid}/content?content=backup")) {
                    Ok(entradas) => {
                        let lista = entradas.as_array().cloned().unwrap_or_default();
                        verificados.push(json!({"node": host, "storage": sid, "returned_backups": lista.len()}));
                        for b in lista {
                            let mut e = serde_json::Map::new();
                            for k in ["volid", "vmid", "size", "ctime", "format", "content", "protected"] {
                                if let Some(v) = b.get(k) {
                                    e.insert(k.into(), v.clone());
                                }
                            }
                            e.insert("node".into(), json!(host));
                            e.insert("storage".into(), json!(sid));
                            e.insert("storage_type".into(), s["type"].clone());
                            e.insert("shared".into(), s.get("shared").cloned().unwrap_or(json!(0)));
                            if let Some(t) = b["ctime"].as_i64().filter(|t| *t > 0).and_then(data_utc) {
                                e.insert("created_utc".into(), json!(t));
                            }
                            itens.push(Value::Object(e));
                        }
                    }
                    Err(e) => erro(&format!("{host}/{sid}"), &e, &mut erros),
                }
            }
        }
        nota = "size é o tamanho informado pela API para o backup. Em PBS não representa necessariamente espaço físico exclusivo após deduplicação. Um storage compartilhado pode aparecer em mais de um nó; não some duplicados.";
    } else {
        let convidados: Vec<Value> = cli
            .dados("/cluster/resources?type=vm")?
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|g| no.is_none_or(|n| g["node"] == n))
            .collect();
        truncado = convidados.len() > 40;
        for g in convidados.iter().take(40) {
            let host = g["node"].as_str().unwrap_or("");
            let tipo = g["type"].as_str().unwrap_or("");
            let vmid = match &g["vmid"] {
                Value::Number(n) => n.to_string(),
                Value::String(s) => s.clone(),
                _ => String::new(),
            };
            if !nome_valido(host)
                || !matches!(tipo, "qemu" | "lxc")
                || vmid.is_empty()
                || vmid.len() > 10
                || !vmid.chars().all(|c| c.is_ascii_digit())
            {
                return Err("VM inválida retornada pela API.".into());
            }
            let config = match cli.dados(&format!("/nodes/{host}/{tipo}/{vmid}/config?current=1")) {
                Ok(c) => c,
                Err(e) => {
                    erro(&format!("{host}/{vmid}"), &e, &mut erros);
                    continue;
                }
            };
            for (slot, spec) in config.as_object().into_iter().flatten() {
                let prefixo = slot.trim_end_matches(|c: char| c.is_ascii_digit());
                let tem_numero = prefixo.len() < slot.len();
                let disco = slot == "rootfs"
                    || (tem_numero
                        && matches!(
                            prefixo,
                            "scsi" | "sata" | "virtio" | "ide" | "efidisk" | "tpmstate" | "unused" | "mp"
                        ));
                if !disco {
                    continue;
                }
                let spec = match spec {
                    Value::String(s) => s.clone(),
                    outro => outro.to_string(),
                };
                let partes: Vec<&str> = spec.split(',').collect();
                let opcoes: std::collections::HashMap<&str, &str> =
                    partes[1..].iter().filter_map(|p| p.split_once('=')).collect();
                let volume = partes[0].strip_prefix("file=").unwrap_or(partes[0]).to_string();
                let mut papel = if opcoes.get("media") == Some(&"cdrom") {
                    "cdrom"
                } else if prefixo == "unused" {
                    "unused"
                } else if prefixo == "efidisk" {
                    "efi"
                } else if prefixo == "tpmstate" {
                    "tpm"
                } else {
                    "disk"
                };
                if tipo == "lxc" && volume.starts_with('/') {
                    papel = if volume.starts_with("/dev/") { "device_mount" } else { "bind_mount" };
                }
                let tamanho = opcoes.get("size").copied();
                let bytes = tamanho.and_then(|t| {
                    let (num, unidade) = match t.chars().last() {
                        Some(u) if u.is_ascii_alphabetic() => (&t[..t.len() - 1], Some(u)),
                        _ => (t, None),
                    };
                    let n: f64 = num.parse().ok()?;
                    let exp = match unidade {
                        None => 0,
                        Some(u) => "KMGTPE".find(u)? as i32 + 1,
                    };
                    Some((n * 1024f64.powi(exp)) as u64)
                });
                itens.push(json!({
                    "node": host, "vmid": vmid.parse::<u64>().unwrap_or(0), "name": g["name"],
                    "guest_type": tipo, "slot": slot, "role": papel, "volume": volume,
                    "configured_size": tamanho, "provisioned_bytes": bytes, "guest_used_bytes": null,
                    "mountpoint": opcoes.get("mp"), "backup": opcoes.get("backup"),
                }));
            }
        }
        nota = "Discos da configuração ATUAL, separados por slot. provisioned_bytes é capacidade provisionada, não uso físico nem ocupação do sistema convidado. null significa não informado. CD-ROM, EFI, TPM e volumes unused são identificados separadamente.";
    }
    let total = itens.len();
    itens.truncate(200);
    Ok(json!({
        "items": itens, "total_items": total, "truncated": truncado || total > 200,
        "partial": !erros.is_empty() || truncado || total > 200, "errors": erros,
        "checked_storages": verificados,
        "measurement_note": format!("{nota} Resultados são limitados ao que a credencial atual pode visualizar."),
    }))
}

/// Consulta somente leitura ao Proxmox configurado.
pub fn consultar(c: &Caminhos, consulta: &str, no: Option<&str>) -> Result<Value, String> {
    let (cli, url) = Cliente::abrir(c)?;
    let dados = if matches!(consulta, "backups" | "discos") {
        caminho("nos", no)?;
        detalhes(&cli, consulta, no)?
    } else {
        sanitizar(cli.dados(&caminho(consulta, no)?)?)
    };
    Ok(json!({
        "source": url,
        "queried_at": chrono::Utc::now().to_rfc3339(),
        "query": consulta,
        "node": no,
        "readonly": true,
        "data": dados,
    }))
}
