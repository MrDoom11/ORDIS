# Proxmox: acesso somente leitura

O agente consulta o Proxmox VE pela API, só com leitura, usando um token próprio com o papel `PVEAuditor`.

## 1. Criar o usuário e o token no Proxmox

1. **Datacenter › Permissions › Users › Add**: usuário `ordis`, realm `pve`. O identificador será `ordis@pve`. Não use root.
2. **Datacenter › Permissions › Add › User Permission**: caminho `/`, usuário `ordis@pve`, papel `PVEAuditor`, *Propagate* marcado.
3. **Datacenter › Permissions › API Tokens › Add**: usuário `ordis@pve`, Token ID `ordis`, *Privilege Separation* marcado. Guarde o segredo, mostrado uma única vez.
4. **Permissions › Add › API Token Permission**: caminho `/`, token `ordis@pve!ordis`, papel `PVEAuditor`, *Propagate* marcado. Com a separação de privilégios, usuário e token precisam da permissão.

## 2. Exportar o certificado da CA

No shell do Proxmox:

```sh
cat /etc/pve/pve-root-ca.pem
```

Salve o conteúdo, de `BEGIN CERTIFICATE` a `END CERTIFICATE`, num arquivo `pve-root-ca.pem`. É o certificado público da CA do cluster, não uma chave privada (não copie o `pve-root-ca.key`). Se o servidor usa um certificado emitido por outra CA, use a cadeia dessa CA.

## 3. Configurar no ORDIS

Em **Configurações › Proxmox**, informe o endereço (por exemplo `https://proxmox.local:8006`), o ID do token, o segredo e o arquivo da CA, e clique em **Salvar e testar**. O segredo é cifrado com DPAPI para o seu usuário do Windows e gravado em `runtime\proxmox`. O teste faz um `GET /nodes`.

Se houver erro de nome no certificado, use o nome de host que consta nele ou corrija o certificado; o ORDIS não desativa a verificação TLS.

## Consultas disponíveis

A ferramenta `proxmox_consultar` aceita `versao`, `nos`, `resumo`, `vms`, `armazenamento`, `estado_no`, `backups` e `discos`. O parâmetro `no` restringe ao nó informado e é obrigatório em `estado_no`.

- Só `GET` em caminhos predefinidos: sem URL arbitrária, comandos, início ou parada de VMs.
- TLS confiando só na CA configurada, sem redirecionamentos, com até 50 chamadas e 2 MB por consulta.
- Os campos devolvidos ao modelo passam por uma lista permitida; não há acesso aos sistemas convidados, então o uso de disco dentro das VMs fica desconhecido.

A limitação no cliente é uma segunda camada: mantenha também o papel `PVEAuditor` no servidor.
