# CowPaper 开发环境变量（每次新 shell 构建前 source 本文件）
export RUSTUP_HOME="$PWD/.rustup"
export CARGO_HOME="$PWD/.cargo"
export PATH="$CARGO_HOME/bin:$PATH"
export RUSTUP_DIST_SERVER="https://rsproxy.cn"
export RUSTUP_UPDATE_ROOT="https://rsproxy.cn/rustup"
export npm_config_cache="$PWD/.npm-cache"
