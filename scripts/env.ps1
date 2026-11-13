# Windows PowerShell 용 빌드 환경 변수 (scripts/env.sh 와 동일한 내용).
#   사용: . .\scripts\env.ps1
$env:RUSTUP_HOME = "C:\rust\rustup"
$env:CARGO_HOME = "C:\rust\cargo"
$env:CARGO_TARGET_DIR = "C:\bs-target"
$env:TEMP = "C:\rust\tmp"
$env:TMP = "C:\rust\tmp"
$env:Path = "C:\rust\cargo\bin;C:\rust\mingw64\bin;" + $env:Path
