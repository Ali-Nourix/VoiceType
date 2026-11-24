$ErrorActionPreference = 'Continue'
$dir = Join-Path $PWD 'qa-output'; New-Item -ItemType Directory -Force $dir | Out-Null
$log = Join-Path $dir 'app.log'; $report = Join-Path $dir 'report.txt'
$env:VOICETYPE_UI_TEST_FAKE='1'; $env:VOICETYPE_UI_TEST_LOG=$log
$env:APPDATA = Join-Path $env:RUNNER_TEMP 'VoiceType-smoke-appdata'
New-Item -ItemType Directory -Force $env:APPDATA | Out-Null
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class WinQA {
 [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, string title);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
 [DllImport("user32.dll")] public static extern void keybd_event(byte key, byte scan, uint flags, UIntPtr extra);
 public struct RECT { public int L,T,R,B; }
}
'@
function note($s) { Add-Content -Path $report -Value $s; Write-Host $s }
function waitlog($pattern, $timeout=12) { $end=(Get-Date).AddSeconds($timeout); do { if ((Test-Path $log) -and (Select-String -Path $log -Pattern $pattern -Quiet)) {return $true}; Start-Sleep -Milliseconds 150 } while ((Get-Date) -lt $end); return $false }
function chord { [WinQA]::keybd_event(0x12,0,0,[UIntPtr]::Zero); Start-Sleep -Milliseconds 90; [WinQA]::keybd_event(0x5A,0,0,[UIntPtr]::Zero); Start-Sleep -Milliseconds 100; [WinQA]::keybd_event(0x5A,0,2,[UIntPtr]::Zero); [WinQA]::keybd_event(0x12,0,2,[UIntPtr]::Zero); }
function snapshot([IntPtr]$h,[string]$path) { try { $r=New-Object WinQA+RECT; if (-not [WinQA]::GetWindowRect($h,[ref]$r)) {return $false}; $w=$r.R-$r.L; $height=$r.B-$r.T; if ($w -le 0 -or $height -le 0 -or $w -gt 4096 -or $height -gt 4096) {return $false}; $bitmap=New-Object System.Drawing.Bitmap($w,$height); $g=[System.Drawing.Graphics]::FromImage($bitmap); $dc=$g.GetHdc(); $ok=[WinQA]::PrintWindow($h,$dc,2); $g.ReleaseHdc($dc); $g.Dispose(); $bitmap.Save($path,[System.Drawing.Imaging.ImageFormat]::Png); $bitmap.Dispose(); return $ok } catch {return $false} }
$watch=[System.Diagnostics.Stopwatch]::StartNew(); $p=Start-Process -FilePath (Join-Path $PWD 'target/release/voicetype.exe') -PassThru
try {
 if (-not (waitlog 'first frame' 45)) {note 'FAIL: first frame was not reached (possibly no interactive desktop/OpenGL).'; return}
 note "first-frame-ms=$($watch.ElapsedMilliseconds) (hosted Windows runner, cold start; not a user PC benchmark)"
 $root=[WinQA]::FindWindow($null,'VoiceType'); note "root-found=$($root -ne [IntPtr]::Zero)"
 if (Test-Path $log) { note ((Get-Content $log | Select-String 'ready ' | Select-Object -First 1).Line) }
 note "home-screenshot=$([bool](snapshot $root (Join-Path $dir 'home.png')))"
 chord
 $started=waitlog 'recording started' 12; note "shortcut-visible-start=$started"
 if ($started) { Start-Sleep -Milliseconds 700; $bar=[WinQA]::FindWindow($null,'VoiceType Floating Bar'); note "bar-found=$($bar -ne [IntPtr]::Zero)"; if ($bar -ne [IntPtr]::Zero) {note "bar-screenshot=$([bool](snapshot $bar (Join-Path $dir 'bar.png')))"}; chord; note "shortcut-visible-stop=$(waitlog 'recording finished' 12)" }
 [WinQA]::PostMessage($root,0x10,[IntPtr]::Zero,[IntPtr]::Zero) | Out-Null; Start-Sleep -Seconds 2
 $hidden= -not [WinQA]::IsWindowVisible($root); note "window-hides=$hidden"
 if ($hidden) {
   if (Test-Path $log) { Remove-Item $log -Force }
   chord; note "shortcut-hidden-start=$(waitlog 'recording started' 12)"
   $tray=[WinQA]::FindWindow('tray_icon_app',$null); note "tray-window-found=$($tray -ne [IntPtr]::Zero)"
   if ($tray -ne [IntPtr]::Zero) { [WinQA]::PostMessage($tray,6002,[IntPtr]::Zero,[IntPtr]0x202) | Out-Null; Start-Sleep -Seconds 3; note "tray-reopens-window=$([WinQA]::IsWindowVisible($root))"; note "tray-callback-handled=$(waitlog 'tray event' 5)" }
 }
} catch {note "ERROR: $($_.Exception.Message)"} finally { if ($p -and -not $p.HasExited) {Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue} }
