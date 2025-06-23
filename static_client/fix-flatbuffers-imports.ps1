# PowerShell script to fix flatbuffers imports in generated protocol files
$protocolPath = "massive_game_server/static_client/generated_js/game-protocol"
$files = Get-ChildItem -Path $protocolPath -Filter "*.js" -Recurse

foreach ($file in $files) {
    $content = Get-Content $file.FullName -Raw
    $newContent = $content -replace "import \* as flatbuffers from 'flatbuffers';", "import * as flatbuffers from 'https://cdn.jsdelivr.net/npm/flatbuffers@23.5.26/+esm';"
    
    if ($content -ne $newContent) {
        Set-Content -Path $file.FullName -Value $newContent
        Write-Host "Fixed imports in: $($file.Name)"
    }
}

Write-Host "Flatbuffers imports fixed in all protocol files."
