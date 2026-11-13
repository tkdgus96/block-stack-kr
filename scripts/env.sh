# Windows(Git Bash) + 한글 사용자 경로 환경용 빌드 환경 변수.
# GNU 툴체인(MinGW ld/dlltool)은 비ASCII 경로(C:\Users\한글\...)를 처리하지 못하므로
# 툴체인·빌드 산출물·임시 폴더를 모두 ASCII 경로에 둔다.
#   사용: . scripts/env.sh
# MSVC 툴체인(Visual Studio Build Tools)을 쓰거나 경로가 ASCII 라면 필요 없다.
export RUSTUP_HOME="C:/rust/rustup" CARGO_HOME="C:/rust/cargo" CARGO_TARGET_DIR="C:/bs-target"
export TEMP="C:\rust\tmp" TMP="C:\rust\tmp"
export PATH="/c/rust/cargo/bin:/c/rust/mingw64/bin:$PATH"
