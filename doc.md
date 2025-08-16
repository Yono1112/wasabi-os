
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

---

## 直線の描写
### 直線描画の全体フロー

1. まず `efi_main` から描画命令を出す

* グリッドや放射状の線を引くために、`draw_line(&mut vram, color, x0, y0, x1, y1)` を何度も呼んでいます。
* この時点では「どのピクセルに塗るか」は `draw_line` にお任せ。`vram` は“2D画像としてのVRAMビュー”。

2. `draw_line` が「線上の各ピクセル座標」を列挙する

* 始点 `(x0, y0)` と終点 `(x1, y1)` から、**差分と進む向き**を出します:

  ```rust
  let dx = (x1 - x0).abs();   // X 方向に何ピクセル進むか（非負）
  let sx = (x1 - x0).signum();// X 方向の符号（-1 / 0 / +1）
  let dy = (y1 - y0).abs();   // Y 方向に何ピクセル進むか（非負）
  let sy = (y1 - y0).signum();// Y 方向の符号
  ```
* どちらの変化量が大きいかで「**主軸**」を決めます。

  * `dx >= dy` → **X を1ずつ進め**、それに対応する Y を補間
  * `dx <  dy` → **Y を1ずつ進め**、それに対応する X を補間

3. 主軸に沿って1ピクセルずつ進み、副軸は `calc_slope_point` で補間

* 例：`dx >= dy` のとき

  ```rust
  for rx in 0..dx {
      if let Some(ry) = calc_slope_point(dx, dy, rx) {
          // (rx, ry) は主軸・副軸の“相対座標”
          let px = x0 + rx * sx;
          let py = y0 + ry * sy;
          draw_point(buf, color, px, py)?;
      }
  }
  ```
* `calc_slope_point(da, db, ia)` は、**主軸長=da**、**副軸長=db**、**主軸の進行量=ia** から
  副軸の相対位置（整数ピクセル）を計算して `Some(ry)` を返します（範囲外なら `None`）。

  * 中身は「分数傾きを固定小数点っぽく丸める」ミニ関数（Bresenhamライク）。

4. 各点の実塗りは `draw_point` が担当

* `draw_point` は **範囲チェック付き**で安全に 1 ピクセルを塗ります：

  ```rust
  fn draw_point<T: Bitmap>(buf: &mut T, color: u32, x: i64, y: i64) -> Result<()> {
      *(buf.pixel_at_mut(x, y).ok_or("Out of Range")?) = color;
      Ok(())
  }
  ```
* `pixel_at_mut` が `(x,y)` の妥当性を見て、フレームバッファ上の該当アドレス（`&mut u32`）を返してくれます。
  ここで「**2次元座標 → 1次元バッファのオフセット**」の変換式が効いています：

  ```rust
  offset_bytes = ((y * pixels_per_line) + x) * bytes_per_pixel
  ```

---

