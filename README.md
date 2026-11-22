# RenamePhotosToTimestamp
## 概要
写真ファイルのファイル名を撮影日時にリネームします。  
GUI 版（`app/`、Windows・macOS・Linux 対応）と、従来の CLI 版（`rename_picture_files_to_timestamp.py`）があります。  
連写撮影した写真は撮影日時が衝突することがあるので、撮影日時の後に元のファイル名が続きます。  
例：7M400001.ARW => IMG_20230223_125312_7M400001.ARW  

## GUI 版（Tauri アプリ）
### できること
- ウィンドウにファイルやフォルダをドラッグ＆ドロップして読み込み（サブフォルダも含める／直下のみを選択可）
- 撮影日時（`DateTimeOriginal`、なければ `DateTimeDigitized`/`CreateDate`）を読み取り、ネイティブで読めないファイルは `PATH` 上の ExifTool で補完（Windows でも HIF を処理可能）
- テンプレートで命名を指定し、変更後の名前を実行前に一覧でプレビュー
  - 初期値：`IMG_{date:YYYYMMDD_HHmmss}_{name}`（CLI 版と同じ名前になります）
  - トークン：`{date:FORMAT}`（`YYYY` `YY` `MM` `DD` `HH` `mm` `ss`）、`{name}`（元の名前から拡張子を除いた部分）、`{seq:N}`（N 桁の連番）、`{camera}`（機種）。それ以外の文字は固定文字として入ります（`{{` `}}` で波かっこそのもの）
- 状態（OK・連番付与・衝突・日時なし・変更なし・除外・エラー・完了）での絞り込み、チェックでの除外、ダブルクリックでの手動編集、選択したファイルの EXIF 情報の表示
- 事前の衝突チェック：同じフォルダ内の重複（大文字・小文字の違いも含む）、既存ファイルとの衝突、使えない文字や Windows の予約名。衝突時は `_001` の連番を付けるかスキップするかを選択
- 1件ずつ順番にリネームし、進み具合・件数・処理中のファイル・処理速度・残り時間・ログを表示。キャンセルも可能
- 既存ファイルは決して上書きしません。実行ごとに履歴を保存し、履歴の画面から一括で元に戻せます。アプリが途中で終了した場合は、次回起動時に「続きから再開」か「元に戻す」かを選べます

### 必要な環境
- Node.js 20 以降
- Rust（stable）。Windows では Visual Studio の C++ ビルドツール（MSVC）と WebView2 も必要です
- Linux では [Tauri の前提パッケージ](https://v2.tauri.app/start/prerequisites/) が必要です
- ネイティブで読めない一部 RAW 形式（CR3・RAF・RW2・ORF 等）の補完に ExifTool を使いたい場合は、`exiftool` を別途インストールして `PATH` を通してください（見つからない場合は ExifTool を使わずに処理を続けます）

### ビルドと起動
```
cd app
npm install
npm run tauri dev      # 開発用に起動
npm run tauri build    # 配布用のインストーラを作成
```
テスト：
```
cd app/src-tauri
cargo test
cargo test --release --test perf -- --ignored --nocapture   # 2万件の処理時間の計測
```

### 使い方
1. アプリを起動し、写真ファイルやフォルダをウィンドウにドロップします。
2. テンプレートや衝突時の扱いを調整し、一覧で変更後の名前を確認します。必要に応じて除外や手動編集をします。
3. 「N 件をリネーム」を押して実行します。
4. 元に戻したいときは「履歴」から対象の実行を選び、「元に戻す」を押します。不要になった履歴は個別削除・一括削除ができます。

### ポータブルモードとデータの保存先
- **通常（インストール時）**：履歴は OS 標準のアプリデータフォルダ内の `journals/` に JSONL 形式で保存されます（Windows の例：`%APPDATA%\com.renamepictures.timestamp\journals`）。
- **ポータブルモード**：実行ファイルと同じフォルダ（macOS では `.app` バンドルと同じフォルダ）に空の `portable` ファイル（または `portable.txt`、あるいは `data/` フォルダ）を置いて起動すると、履歴と WebView データが同じ場所の `data/` 配下（`data/journals`・`data/webview`）にまとめて保存されます。
  - `portable` ファイルを残しておけば、`data/` フォルダを削除しても次回以降もポータブルモードのまま利用できます。
  - 不要になったらアプリフォルダごと（または `data/` フォルダを）削除することでシステム領域に履歴や WebView2 のデータを残さず片付けられます。
- **環境変数での指定**：環境変数 `RENAME_PHOTOS_TO_TIMESTAMP_GUI_DATA_DIR` を指定すると、そのフォルダ配下（`journals/`・`webview/`）をデータ保存先として使用します。
- 現在の動作モードと保存先パスは「履歴」画面で確認でき、そのままフォルダを開くこともできます。

### 補足
- ExifTool は同梱していません。ネイティブで読み取れなかったファイルがある場合のみ、`PATH` 上の `exiftool` を補助的に呼び出します。
- CLI 版とは異なり、名前の途中にある `.` は消さずに残します（例：`a.b.jpg` の `{name}` は `a.b`）。

## CLI 版（Python）
引数で渡したディレクトリにある写真ファイルをすべてリネームします。  
Python 3系で使用してください。  
### 動作確認済みファイル形式
#### 処理可能
- JPEG
- DNG
- ARW
- HIF (macOS上で実行する場合)
#### 処理不可能
macOS以外の環境では以下の形式のファイルを処理することができません（GUI 版では処理できます）。
- HIF（ソニーαシリーズで撮影）
### 使用方法
python rename_picture_files_to_timestamp.py [写真ファイルを配置したパス]  
例：python ./rename_picture_files_to_timestamp.py ./pictures

## Summary
This program renames photo files to shooting date and time.  
It is available as a cross-platform GUI app (`app/`, Tauri) and as the original CLI script (`rename_picture_files_to_timestamp.py`).  
Original file name is appended after shooting date and time because shooting date and time can conflict when drive mode is used.  
Example: 7M400001.ARW => IMG_20230223_125312_7M400001.ARW  
## GUI app
- Drag and drop files or folders into the window, preview the new names, then rename.
- Naming is configured with a template such as `IMG_{date:YYYYMMDD_HHmmss}_{name}` (tokens: `{date:FORMAT}`, `{name}`, `{seq:N}`, `{camera}`; any other text is kept literally).
- Conflicts are detected before renaming, existing files are never overwritten, every run is journaled and can be undone or deleted, and interrupted runs can be resumed.
- Portable mode: place an empty `portable` (or `portable.txt`) file or a `data/` directory next to the executable (or next to the `.app` bundle on macOS) to store journals and WebView data under `data/`. You can also override the data directory with `RENAME_PHOTOS_TO_TIMESTAMP_GUI_DATA_DIR`.
- Build: `cd app && npm install && npm run tauri build` (requires Node.js, Rust and the Tauri prerequisites).
## CLI script
This script renames all photo files in a directory that selected as argument.  
Please use this program with Python 3.  
### Tested file formats
#### Processable
- JPEG
- DNG
- ARW
- HIF　(only when run this program on macOS)
#### Not processable
This program cannot process files of below format on other than macOS (the GUI app can).
- HIF (taken by Sony α series)
### How to use
python rename_picture_files_to_timestamp.py [picture files directory path]  
Example: python ./rename_picture_files_to_timestamp.py ./pictures
