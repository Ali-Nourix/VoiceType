# Recreate the exact dependency lock from the compressed, text-only repository copy.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$encoded = Get-Content -Raw (Join-Path $root 'Cargo.lock.gz.b64')
$bytes = [Convert]::FromBase64String($encoded)
$inputStream = [System.IO.MemoryStream]::new($bytes)
$gzip = [System.IO.Compression.GZipStream]::new($inputStream, [System.IO.Compression.CompressionMode]::Decompress)
$output = [System.IO.File]::Create((Join-Path $root 'Cargo.lock'))
try { $gzip.CopyTo($output) } finally { $output.Dispose(); $gzip.Dispose(); $inputStream.Dispose() }
