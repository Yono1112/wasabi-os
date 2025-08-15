
### (p.36)
```
$ sudo apt install -y build-essential qemu-system-x86 netcat-openbsd
```
はmacOSだと
```
brew install llvm qemu netcat
```
になる

### UEFIを使って画面(フレームバッファ)をいじる方法(p.59~)
1. efi_main() の第2引数 → EFI System Tableのアドレスが取れる。
1. EFI System Table → locate_protocol() 関数ポインタが入っている。
1. UEFI仕様書 → EFI Graphics Output ProtocolのGUIDが載っている。
1. GUIDをlocate_protocol()に渡す → プロトコル構造体のポインタが得られる。
1. プロトコル構造体の中 → フレームバッファのアドレスや画面サイズ情報がある。
1. フレームバッファを操作 → 画面に図形や文字を描ける。
