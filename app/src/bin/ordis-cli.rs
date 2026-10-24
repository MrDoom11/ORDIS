//! `ordis-cli estado|iniciar|parar|perguntar "texto"|proxmox <consulta> [nó]|imagem "prompt"|benchmark [opções]`.
//! Executável de console separado, porque o ORDIS.exe é de janela e o
//! terminal não espera por ele.
#![allow(dead_code)]

#[path = "../agente.rs"]
mod agente;
#[path = "../benchmark.rs"]
mod benchmark;
#[path = "../controle.rs"]
mod controle;
#[path = "../ferramentas.rs"]
mod ferramentas;
#[path = "../imagens.rs"]
mod imagens;
#[path = "../modelos.rs"]
mod modelos;
#[path = "../proxmox.rs"]
mod proxmox;
#[path = "../win.rs"]
mod win;

use std::io::Write;
use std::sync::atomic::AtomicBool;

use serde_json::Value;

struct Terminal;

impl agente::Saida for Terminal {
    fn evento(&self, tipo: &str, dados: Value) {
        let t = dados["texto"].as_str().unwrap_or("");
        match tipo {
            "raciocinio" => eprint!("{t}"),
            "texto" => print!("{t}"),
            "ferramenta" => println!("\n[ferramenta] {} {}", dados["nome"], dados["argumentos"]),
            "resultado" => println!("[resultado] {}", t.chars().take(300).collect::<String>()),
            "status" => println!("[status] {t}"),
            _ => {}
        }
        let _ = std::io::stdout().flush();
    }
}

fn executar(c: &controle::Caminhos, acao: &str) -> Result<(), String> {
    let imprimir = |m: &str| println!("{m}");
    match acao {
        "iniciar" => controle::iniciar(c, &imprimir)?,
        "parar" => controle::parar(c, &imprimir)?,
        "estado" => {}
        "perguntar" => {
            let texto = std::env::args().skip(2).collect::<Vec<_>>().join(" ");
            let s = agente::nova(c)?;
            agente::responder(c, &s.id, texto, vec![], &AtomicBool::new(false), &Terminal)?;
            println!("\n[sessão {}]", s.id);
            return Ok(());
        }
        "proxmox" => {
            let args: Vec<String> = std::env::args().skip(2).collect();
            let consulta = args.first().map(String::as_str).unwrap_or("resumo");
            let v = proxmox::consultar(c, consulta, args.get(1).map(String::as_str))?;
            println!("{}", serde_json::to_string_pretty(&v).unwrap());
            return Ok(());
        }
        "benchmark" => {
            let args: Vec<String> = std::env::args().skip(2).collect();
            return benchmark::executar(c, benchmark::opcoes(&args)?);
        }
        "imagem" => {
            let prompt = std::env::args().skip(2).collect::<Vec<_>>().join(" ");
            let v = imagens::gerar(c, &serde_json::json!({"prompt": prompt}), &AtomicBool::new(false), &|s| {
                println!("[{}] {}", s["stage"].as_str().unwrap_or(""), s["message"].as_str().unwrap_or(""));
            });
            println!("{v}");
            return if v["ok"] == true { Ok(()) } else { Err(v["error"].as_str().unwrap_or("falha").into()) };
        }
        _ => {
            return Err(format!(
                "Ação desconhecida: {acao}. Use estado, iniciar, parar, perguntar, proxmox, imagem ou benchmark."
            ))
        }
    }
    let p = controle::painel(c);
    for s in &p.servicos {
        println!("{}: {}", s.nome, s.situacao);
    }
    println!("VRAM: {}", p.vram.as_deref().unwrap_or("indisponível"));
    Ok(())
}

fn main() {
    let acao = std::env::args().nth(1).unwrap_or_else(|| "estado".into());
    let r = controle::Caminhos::descobrir().and_then(|c| executar(&c, &acao));
    if let Err(e) = r {
        eprintln!("ERRO: {e}");
        std::process::exit(1);
    }
}
