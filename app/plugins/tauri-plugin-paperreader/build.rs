// 這個外掛沒有給網頁直接呼叫的指令：網頁只呼叫應用自己的指令（`src-tauri`），
// 應用的指令再經由 `PaperReaderExt` 呼叫這個外掛。所以指令清單是空的。
const COMMANDS: &[&str] = &[];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
