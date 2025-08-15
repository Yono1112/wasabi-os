
## brewでのインストール方法(p.36)
```
$ sudo apt install -y build-essential qemu-system-x86 netcat-openbsd
```
はmacOSだと
```
brew install llvm qemu netcat
```
になる

## UEFIを使って画面(フレームバッファ)をいじる方法(p.59~)
1. efi_main() の第2引数 → EFI System Tableのアドレスが取れる。
1. EFI System Table → locate_protocol() 関数ポインタが入っている。
1. UEFI仕様書 → EFI Graphics Output ProtocolのGUIDが載っている。
1. GUIDをlocate_protocol()に渡す → プロトコル構造体のポインタが得られる。
1. プロトコル構造体の中 → フレームバッファのアドレスや画面サイズ情報がある。
1. フレームバッファを操作 → 画面に図形や文字を描ける。

## 図形を描く方法(p.72~)
### 全体像
- 画面＝2次元のピクセル配列
- フレームバッファ＝その2次元配列を1次元メモリにフラット化したもの
- 任意の点 (x, y) を塗るには：
  - オフセット = (y * 行あたりのピクセル数 + x) * 1ピクセルのバイト数 を出して、そのアドレスに色を書けばよい
- →これを安全・再利用可能にするために、2D画像を抽象化する Bitmap トレイトを作り、その実装として VRAMの実体 VramBufferInfo を用意しています。
### 画面メモリ(フレームバッファ)の考え方
- 1ピクセル = 4バイト（本コードでは固定：bytes_per_pixel() = 4）
  - 先頭4バイト … 左上 (0,0) の色
  - 次の4バイト … (1,0) の色
  - … (width-1,0) までが1行目
  - その次から2行目 (0,1) が始まる
- ただし、行あたりの実際のピクセル数 = pixels_per_scan_line（Stride）なので、画面の表示幅 width と一致しないことがある（右端にパディングが入るGPUがある）
- →よって、オフセット計算は y * pixels_per_line + x を使うのが正解。単純に y * width + x だと一部環境で崩れます。
