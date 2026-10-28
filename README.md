# HALLS

第一人称后室终端迷宫。地图、实体、材质和音效均由程序生成，通关后可导出加密签名的 `.hproof` 凭证。支持 Windows x86_64、macOS ARM64 / x86_64、Linux ARM64 / x86_64。

## 运行

使用支持真彩色和 VT 控制序列的终端，窗口至少 32×12。WASD 移动，方向键转向，Z 切换奔跑，F 切换手电筒，M 静音，`[` / `]` 调节音量，Esc 暂停。分辨率在菜单中设置。没有按键释放事件的终端使用短步输入。

通关凭证保存在程序所在目录，该目录需要可写；已有同名文件不会被覆盖。客户端离线运行，不会自动上传凭证。

## 构建

需要 Rust 1.88+ 和 C 编译器。Windows 使用 Visual Studio 的 x64 Native Tools 环境；macOS 使用 Xcode Command Line Tools，运行系统需 macOS 13+；Linux 需要 OpenSSL 3 开发库，声音使用系统 ALSA，可在无音频设备时继续运行。

先用 Python 3.10+ 生成一套独立密钥，目录必须位于仓库外且尚不存在：

```sh
python -m pip install -r requirements-dev.txt
python tools/proof/keygen.py /path/outside/repository/halls-keys
```

构建时指定生成的签名头文件：

```sh
export HALLS_PROOF_KEYS=/path/outside/repository/halls-keys/proof_keys.h
cargo build --release --locked --bin halls
```

PowerShell 使用：

```powershell
$env:HALLS_PROOF_KEYS = 'C:\private\halls-keys\proof_keys.h'
cargo build --release --locked --bin halls
```

程序位于 `target/release/halls`（Windows 为 `halls.exe`）。发行包只需程序和 `LICENSE`；Linux 运行时还需系统提供 `libcrypto.so.3`。macOS 可分别通过 `--target aarch64-apple-darwin` 和 `--target x86_64-apple-darwin` 构建，目标需预先安装。

`proof_keys.h` 包含客户端签名材料；`recipient.pem` 是组织者解密私钥，不能随客户端分发。`trust.json` 供组织者校验签名，三者必须来自同一套密钥。不要提交密钥、录制、凭证或构建目录；重新生成密钥不会验证此前环境的凭证。

## 验证

设置同一个 `HALLS_PROOF_KEYS` 后运行：

```sh
cargo fmt --check
cargo test --release --all-targets --features tools
cargo clippy --all-targets --features tools -- -D warnings
python tools/compile_entity.py --check
python -m unittest discover -s tests -p 'test_*.py'
cargo run --release --features tools --example proof_fixture -- output/proof
cargo run --release --features tools --example crypto_probe -- output/crypto
python tools/check_proof.py --keys /path/outside/repository/halls-keys --fixture output/proof --probe output/crypto
```

macOS / Linux 的终端交互测试：

```sh
cargo build --release --features tools --bin halls-tools
cargo build --release --bin halls --example panic_restore
HALLS_PANIC_BINARY=target/release/examples/panic_restore python tools/check_terminal.py
```

`tools` 特性提供地图、渲染和判题夹具；它会给录制加上开发标记，不用于玩家发行。CI 在三个操作系统上使用临时密钥验证构建、录制和密码学边界，不发布客户端。

## 凭证与校验

录制逐 tick 保存模拟状态、独立系统时钟、电池和出口坐标，使用 ECDSA P-256 签名、AES-256-GCM 加密和 RSA-OAEP-SHA256 封装。历史录制在内存中加密，导出时只解密有界工作区。

离线签名材料仍可从客户端提取，签名有效不等于可信执行或可信计时。组织者应独立重建地图、复算轨迹并检查计时与出口规则。协议见 [docs/PROOF.md](docs/PROOF.md)，成绩管理与回放见 [HALLS 平台](https://github.com/ZackMount/halls-platform)。

[MIT License](LICENSE)
