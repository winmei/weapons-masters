param(
    [string]$GodotExecutable = ""
)

$ErrorActionPreference = "Stop"
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$clientRoot = Join-Path $projectRoot "client"
$scenePath = "res://scenes/AeriviaTest.tscn"
$glbPath = Join-Path $clientRoot "assets\maps\aerivia\aerivia_blockout.glb"
$importSidecar = "$glbPath.import"
$importLog = Join-Path $projectRoot "docs\aerivia_godot_import.log"
$runLog = Join-Path $projectRoot "docs\aerivia_godot_run.log"

function Find-GodotExecutable {
    param([string]$ExplicitPath)

    if ($ExplicitPath) {
        if (-not (Test-Path -LiteralPath $ExplicitPath -PathType Leaf)) {
            throw "Executavel Godot informado nao existe: $ExplicitPath"
        }
        return (Resolve-Path -LiteralPath $ExplicitPath).Path
    }

    if ($env:GODOT_BIN -and (Test-Path -LiteralPath $env:GODOT_BIN -PathType Leaf)) {
        return (Resolve-Path -LiteralPath $env:GODOT_BIN).Path
    }

    foreach ($commandName in @("godot", "godot4")) {
        $command = Get-Command $commandName -ErrorAction SilentlyContinue
        if ($command) {
            return $command.Source
        }
    }

    $searchRoots = @(
        "C:\ProgramasDev",
        "C:\Program Files",
        "C:\Program Files (x86)",
        (Join-Path $env:LOCALAPPDATA "Programs"),
        (Join-Path $env:LOCALAPPDATA "Microsoft\WinGet\Packages"),
        (Join-Path $env:USERPROFILE "Desktop"),
        (Join-Path $env:USERPROFILE "Downloads")
    )
    foreach ($root in $searchRoots) {
        if (-not (Test-Path -LiteralPath $root)) {
            continue
        }
        $candidate = Get-ChildItem -LiteralPath $root -Filter "Godot*_console.exe" -File -Recurse -ErrorAction SilentlyContinue |
            Sort-Object FullName |
            Select-Object -First 1
        if ($candidate) {
            return $candidate.FullName
        }
    }

    throw "Godot 4 Mono/.NET nao encontrado. Passe -GodotExecutable ou defina GODOT_BIN."
}

$godot = Find-GodotExecutable -ExplicitPath $GodotExecutable
$version = (& $godot --version | Select-Object -First 1).Trim()
if (-not $version) {
    throw "Falha ao consultar a versao da Godot: $godot"
}
if ($version -notmatch "^4\.") {
    throw "E necessario Godot 4.x; encontrado: $version"
}
if ($version -notmatch "mono") {
    throw "O projeto usa C#. Use uma build Godot Mono/.NET; encontrado: $version"
}

Write-Output "GODOT_EXECUTABLE=$godot"
Write-Output "GODOT_VERSION=$version"

$needsImport = -not (Test-Path -LiteralPath $importSidecar) -or
    (Get-Item -LiteralPath $importSidecar).LastWriteTimeUtc -lt (Get-Item -LiteralPath $glbPath).LastWriteTimeUtc
if ($needsImport) {
    & $godot --headless --rendering-method gl_compatibility --path $clientRoot --import --log-file $importLog
    if ($LASTEXITCODE -ne 0) {
        throw "A importacao Godot falhou. Consulte: $importLog"
    }
} else {
    Write-Output "AERIVIA_IMPORT_UP_TO_DATE=True"
}

& $godot --headless --rendering-method gl_compatibility --path $clientRoot --scene $scenePath --quit-after 300 --log-file $runLog -- --aerivia-validation
if ($LASTEXITCODE -ne 0) {
    throw "A cena de teste falhou. Consulte: $runLog"
}

$runOutput = Get-Content -Raw -LiteralPath $runLog
$requiredMarkers = @(
    "AERIVIA_TEST_MAP_MESHES=105",
    "AERIVIA_TEST_PLAYER_SCALE_OK=True",
    "AERIVIA_TEST_PLAYER_HEIGHT_OK=True",
    "AERIVIA_TEST_CAMERA_OK=True",
    "AERIVIA_TEST_COLLISION_OK=True",
    "AERIVIA_TEST_MATERIALS_OK=True",
    "AERIVIA_TEST_READY"
)
foreach ($marker in $requiredMarkers) {
    if (-not $runOutput.Contains($marker)) {
        throw "Marcador de validacao ausente: $marker"
    }
}

$combinedLogs = (Get-Content -Raw -LiteralPath $importLog) + "`n" + $runOutput
if ($combinedLogs -match "(?im)^.*(SCRIPT ERROR|Parse Error|Failed loading|ERROR:).*$") {
    throw "Erros encontrados nos logs Godot. Consulte $importLog e $runLog."
}

Write-Output "AERIVIA_GODOT_VALIDATION_OK"
