# End-to-end test of editing the weather selection database in the editor and converting it:
#   1. the editor (scripted, --vdb-script) loads the database the game uses, adds a value to a category, renames it, adds
#      a variation and saves both formats into its output folder;
#   2. variation_convert turns the saved .bin into XML and the saved .xml into FASTBIN: both must equal what the editor wrote;
#   3. the saved XML must contain the edit (new value, new variation with every flag bit set).
# Files already in the output folder are backed up and restored. Run from the weather_editor folder after building:
#   powershell -ExecutionPolicy Bypass -File tools\variation_e2e.ps1 [-Config Release]
param([string]$Config = "Release")
$ErrorActionPreference = "Stop"
$bin = Join-Path $PSScriptRoot "..\build\$Config"
$work = Join-Path $env:TEMP "awe_variation_e2e"
Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
New-Item -ItemType Directory $work | Out-Null

# The editor's output folder (Settings menu): <Documents>\AttilaWeatherEditor\out by default.
$out = Join-Path ([Environment]::GetFolderPath("MyDocuments")) "AttilaWeatherEditor\out\warscape_asset_variation_db"
$backup = Join-Path $work "backup"
if (Test-Path $out) { Copy-Item $out $backup -Recurse }
try {
    $script = 'add-value:climate=ROME_E2E;rename-value:climate=ROME_E2E>ROME_E2E2;add-variation:weather\e2e\test.environment;save:both'
    Start-Process (Join-Path $bin "weather_editor.exe") -ArgumentList @("--vdb-script", "`"$script`"", "--shot", (Join-Path $work "shot.png"), "--frames", "40") -Wait
    $savedXml = Join-Path $out "warscape_asset_variation_db.xml"
    $savedBin = Join-Path $out "warscape_asset_variation_db.bin"
    if (-not (Test-Path $savedXml) -or -not (Test-Path $savedBin)) { throw "the editor did not write both files to $out" }
    $convert = Join-Path $bin "variation_convert.exe"
    & $convert $savedBin (Join-Path $work "from_bin.xml") | Out-Null
    & $convert $savedXml (Join-Path $work "from_xml.bin") | Out-Null
    if ((Get-FileHash (Join-Path $work "from_bin.xml")).Hash -ne (Get-FileHash $savedXml).Hash) { throw "converting the saved .bin gives different XML than the editor saved" }
    if ((Get-FileHash (Join-Path $work "from_xml.bin")).Hash -ne (Get-FileHash $savedBin).Hash) { throw "converting the saved .xml gives different FASTBIN than the editor saved" }
    $xml = Get-Content $savedXml -Raw
    if ($xml -notmatch "value='ROME_E2E2'") { throw "the renamed value is not in the saved XML" }
    if ($xml -match "ROME_E2E'") { throw "the old value name is still in the saved XML" }
    if ($xml -notmatch "filename='weather\\e2e\\test.environment' flags='4294967295'") { throw "the new variation is missing or its flags are not all 32 bits" }
    Write-Host "OK: edits saved as XML and FASTBIN, both convert into each other byte for byte, 32 values and the new variation are in the file"
} finally {
    Remove-Item -Recurse -Force $out -ErrorAction SilentlyContinue
    if (Test-Path $backup) { Copy-Item $backup $out -Recurse }
}
