# Compila o app ORDIS e o instala em <projeto>\bin, com os atalhos da
# Área de Trabalho e do menu Iniciar. Rode de novo após cada atualização.
$ErrorActionPreference = 'Stop'
$app = $PSScriptRoot
$projeto = Split-Path $app -Parent
$bin = Join-Path $projeto 'bin'

Push-Location $app
try { cargo build --release; if ($LASTEXITCODE) { throw 'A compilação falhou.' } }
finally { Pop-Location }

if (Get-Process ordis -ErrorAction SilentlyContinue) { throw 'Feche o ORDIS antes de instalar.' }
New-Item -ItemType Directory -Force $bin | Out-Null
$release = Join-Path $app 'target\release'
Copy-Item (Join-Path $release 'ordis.exe') (Join-Path $bin 'ORDIS.exe') -Force
Copy-Item (Join-Path $release 'ordis-cli.exe') $bin -Force
# O toolchain GNU carrega o WebView2 por esta DLL, que precisa ficar ao lado do executável.
Copy-Item (Join-Path $release 'WebView2Loader.dll') $bin -Force

$shell = New-Object -ComObject WScript.Shell
$destinos = @(
    [Environment]::GetFolderPath('Desktop'),
    (Join-Path ([Environment]::GetFolderPath('Programs')) 'ORDIS')
)
foreach ($pasta in $destinos) {
    New-Item -ItemType Directory -Force $pasta | Out-Null
    $atalho = $shell.CreateShortcut((Join-Path $pasta 'ORDIS.lnk'))
    $atalho.TargetPath = Join-Path $bin 'ORDIS.exe'
    $atalho.WorkingDirectory = $projeto
    $atalho.IconLocation = (Join-Path $bin 'ORDIS.exe') + ',0'
    $atalho.Description = 'ORDIS: agente, imagens e painel'
    $atalho.Save()
}
Write-Host "ORDIS instalado em $bin."
