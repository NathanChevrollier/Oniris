// Helpers purs partagés par les scripts de mesure (testés par soak.test.mjs).
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";

// Vrai si le module est le point d'entrée (Windows : séparateurs et casse normalisés).
export const isMain = (metaUrl, argv1) =>
  !!argv1 && resolve(fileURLToPath(metaUrl)).toLowerCase() === resolve(argv1).toLowerCase();

// Script PowerShell : VRAM dédiée (octets) des processus voix/IA (budget) et des pid Oniris (informatif).
// has=false : jeu de compteurs absent (non mesurable) ; un pid sans instance compte 0 (aucun usage GPU).
// String.raw : les antislashs du chemin de compteur doivent arriver intacts à PowerShell.
export const vramScript = (onirisPids) => String.raw`
$ai = @(Get-CimInstance Win32_Process | Where-Object {
  $_.Name -in 'whisper-server.exe','piper.exe','ollama_llama_server.exe','claude.exe' -or
  ($_.Name -eq 'ollama.exe' -and $_.CommandLine -match 'runner') } | ForEach-Object { [int]$_.ProcessId })
$has = $false
try { $null = Get-Counter -ListSet 'GPU Process Memory' -ErrorAction Stop; $has = $true } catch {}
function V($ids) { $s = 0
  foreach ($id in $ids) {
    try { $c = Get-Counter "\GPU Process Memory(pid_$id*)\Dedicated Usage" -ErrorAction Stop
      $s += ($c.CounterSamples | Measure-Object CookedValue -Sum).Sum } catch {}
  }
  $s }
[pscustomobject]@{ has = $has; aiCount = $ai.Count; aiVram = (V $ai); onirisVram = (V @(${onirisPids.join(",")})) } | ConvertTo-Json -Compress`;
