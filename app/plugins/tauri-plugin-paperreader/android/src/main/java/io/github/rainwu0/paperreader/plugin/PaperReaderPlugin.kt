package io.github.rainwu0.paperreader.plugin

import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.CancellationSignal
import android.os.Environment
import android.os.ParcelFileDescriptor
import android.print.PageRange
import android.print.PrintAttributes
import android.print.PrintDocumentAdapter
import android.print.PrintDocumentInfo
import android.print.PrintManager
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.provider.Settings
import android.view.WindowManager
import android.webkit.WebView
import androidx.activity.result.ActivityResult
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.io.IOException
import java.util.UUID
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicInteger
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties

@InvokeArg
class DarkBarsArgs {
    var dark: Boolean = false
}

@InvokeArg
class KeepScreenOnArgs {
    var on: Boolean = false
}

@InvokeArg
class SecretArgs {
    /** 金鑰的名稱：只允許小寫英數字與底線（它就是檔名）。 */
    var name: String = ""
    var value: String = ""
}

@InvokeArg
class PickArgs {
    /** "pdf" 或 "any"。 */
    var kind: String = "pdf"
    var multiple: Boolean = false
}

@InvokeArg
class ExportArgs {
    var path: String = ""
    var name: String = "export.zip"
    var share: Boolean = false
    /** 檔案的種類（分享與存到…用）；沒給就是備份檔的 zip。 */
    var mime: String? = null
}

@InvokeArg
class ClipArgs {
    var text: String = ""
    var html: String? = null
}

@InvokeArg
class PrintArgs {
    var path: String = ""
    var name: String = "document.pdf"
}

@InvokeArg
class OpenUrlArgs {
    var url: String = ""
}

@InvokeArg
class OpenExternalArgs {
    var path: String = ""
    /** "chooser"（系統的「用其他應用打開」選單）或 "obsidian"（obsidian://open?vault=…&file=… 連結）。 */
    var mode: String = "chooser"
}

/**
 * PaperReader 自有的外掛。只做 Android 才有的事，不含任何文獻庫的規則：
 *  - 查「所有檔案存取」權限與各個儲存位置，帶使用者到授權頁
 *  - 開系統的選檔畫面（可以一次選多份），把選到的每一份 PDF 複製成暫存檔交給 Rust
 *  - 收下別的應用分享進來的 PDF（一份或多份），同樣複製成暫存檔，等畫面來領取
 *  - 閱讀時讓螢幕不自動關閉（視窗旗標，不需要權限）
 *  - 回報螢幕邊界（狀態列、瀏海、手勢列）、設定系統列圖示的明暗
 *
 * 外掛沒有開放給網頁的指令；Rust 端經由 `PaperReaderExt` 呼叫。
 * 耗時的檔案複製都在背景執行緒，不佔主執行緒。
 */
@TauriPlugin
class PaperReaderPlugin(private val activity: Activity) : Plugin(activity) {
    private companion object {
        /** 記在分享的 intent 上：這一個已經收過了。 */
        const val SHARE_HANDLED = "io.github.rainwu0.paperreader.share_handled"
    }

    private var webView: WebView? = null
    private var insetTop = 0.0
    private var insetBottom = 0.0
    private var insetLeft = 0.0
    private var insetRight = 0.0

    // ---------- paperreader:// 連結 ----------

    private var pendingLink: String? = null

    private fun rememberLink(intent: Intent?) {
        val uri = intent?.data ?: return
        if (uri.scheme == "paperreader") pendingLink = uri.toString()
    }

    override fun onNewIntent(intent: Intent) {
        rememberLink(intent)
        rememberShare(intent)
    }

    @Command
    fun takeLink(invoke: Invoke) {
        val ret = JSObject()
        ret.put("link", pendingLink)
        pendingLink = null
        invoke.resolve(ret)
    }

    // ---------- 別的應用分享進來的 PDF ----------
    //
    // 收到分享就在背景複製成暫存檔（`cache/shared/<隨機>/<檔名>`）：對方給的讀取授權只在這一次有效。
    // 複製與領取排在同一條單一執行緒的佇列上，所以畫面來領取時，先前收到的都已經複製好，不會漏掉。
    // 畫面先問有幾份（`sharedCount`），文獻庫準備好了再領取（`takeShared`），之後走和系統選檔相同的加入流程。

    private val shareQueue = Executors.newSingleThreadExecutor()
    /** 已複製好、還沒被領取的（只在 `shareQueue` 的執行緒上讀寫）。 */
    private val sharedReady = ArrayList<JSObject>()
    /** 收到、還沒被領取的份數（含還在複製的）。 */
    private val sharedWaiting = AtomicInteger(0)

    private fun sharedRoot(): File = File(activity.cacheDir, "shared")

    private fun rememberShare(intent: Intent?) {
        if (intent == null) return
        val action = intent.action
        if (action != Intent.ACTION_SEND && action != Intent.ACTION_SEND_MULTIPLE) return
        // 同一個 intent 只收一次；從「最近使用」重開時系統會再交一次當初的 intent，那時對方的授權已經沒了
        if (intent.getBooleanExtra(SHARE_HANDLED, false)) return
        if (intent.flags and Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY != 0) return
        intent.putExtra(SHARE_HANDLED, true)
        val uris = sharedUris(intent)
        if (uris.isEmpty()) return
        sharedWaiting.addAndGet(uris.size)
        for (uri in uris) {
            shareQueue.execute { sharedReady.add(copyOne(uri, sharedRoot())) }
        }
    }

    /**
     * 分享進來的檔案網址。只收別的應用經內容提供者交來的（`content://`）：`file://` 可能指向這個應用自己的私有檔案，
     * 自己的檔案提供者交來的也不收（別的應用可以拿這兩種騙我們把私有檔案複製進文獻庫）。
     */
    @Suppress("DEPRECATION")
    private fun sharedUris(intent: Intent): List<Uri> {
        val found = LinkedHashSet<Uri>()
        if (intent.action == Intent.ACTION_SEND_MULTIPLE) {
            val list: ArrayList<Uri>? = if (Build.VERSION.SDK_INT >= 33) {
                intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM, Uri::class.java)
            } else {
                intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM)
            }
            list?.let { found.addAll(it) }
        } else {
            val one: Uri? = if (Build.VERSION.SDK_INT >= 33) {
                intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
            } else {
                intent.getParcelableExtra(Intent.EXTRA_STREAM)
            }
            one?.let { found.add(it) }
        }
        if (found.isEmpty()) {
            intent.clipData?.let { clip ->
                for (k in 0 until clip.itemCount) clip.getItemAt(k).uri?.let { found.add(it) }
            }
        }
        val own = activity.packageName + ".fileprovider"
        return found.filter { it.scheme == "content" && it.authority != own }
    }

    @Command
    fun sharedCount(invoke: Invoke) {
        val ret = JSObject()
        ret.put("count", sharedWaiting.get())
        invoke.resolve(ret)
    }

    /** 交出已收到的分享（每份是暫存檔的路徑，或失敗的種類），交出後清空。排在佇列最後：先前收到的都複製完才交。 */
    @Command
    fun takeShared(invoke: Invoke) {
        shareQueue.execute {
            val files = JSArray()
            for (o in sharedReady) files.put(o)
            sharedWaiting.addAndGet(-sharedReady.size)
            sharedReady.clear()
            val ret = JSObject()
            ret.put("files", files)
            invoke.resolve(ret)
        }
    }

    // ---------- 螢幕邊界 ----------

    override fun load(webView: WebView) {
        this.webView = webView
        rememberLink(activity.intent)
        // 上次被中斷留下的分享暫存先清掉（排在佇列最前面，在處理啟動用的 intent 之前）
        shareQueue.execute { sharedRoot().deleteRecursively() }
        rememberShare(activity.intent)
        val decor = activity.window.decorView
        ViewCompat.setOnApplyWindowInsetsListener(decor) { _, insets ->
            applyInsets(insets)
            insets
        }
        ViewCompat.getRootWindowInsets(decor)?.let { applyInsets(it) }
    }

    /** 記下目前的邊界，並把它寫成網頁的 CSS 變數，讓畫面把內容留在安全範圍裡。 */
    private fun applyInsets(insets: WindowInsetsCompat) {
        val density = activity.resources.displayMetrics.density.toDouble()
        val bars = insets.getInsets(
            WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
        )
        insetTop = bars.top / density
        insetBottom = bars.bottom / density
        insetLeft = bars.left / density
        insetRight = bars.right / density
        val js = "(function(){var s=document.documentElement.style;" +
            "s.setProperty('--inset-top','${insetTop}px');" +
            "s.setProperty('--inset-bottom','${insetBottom}px');" +
            "s.setProperty('--inset-left','${insetLeft}px');" +
            "s.setProperty('--inset-right','${insetRight}px');})()"
        webView?.post { webView?.evaluateJavascript(js, null) }
    }

    @Command
    fun insets(invoke: Invoke) {
        ViewCompat.getRootWindowInsets(activity.window.decorView)?.let { applyInsets(it) }
        val ret = JSObject()
        ret.put("top", insetTop)
        ret.put("bottom", insetBottom)
        ret.put("left", insetLeft)
        ret.put("right", insetRight)
        invoke.resolve(ret)
    }

    @Command
    fun setDarkBars(invoke: Invoke) {
        val args = invoke.parseArgs(DarkBarsArgs::class.java)
        activity.runOnUiThread {
            val controller = WindowCompat.getInsetsController(activity.window, activity.window.decorView)
            controller.isAppearanceLightStatusBars = !args.dark
            controller.isAppearanceLightNavigationBars = !args.dark
            invoke.resolve()
        }
    }

    // ---------- 儲存位置與權限 ----------

    private fun hasAllFiles(): Boolean =
        Build.VERSION.SDK_INT >= 30 && Environment.isExternalStorageManager()

    @Suppress("DEPRECATION")
    private fun sharedDir(): File =
        File(Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOCUMENTS), "PaperReader")

    /**
     * 應用專屬空間裡的文獻庫位置。外部空間暫時取不到時回報失敗，不改用內部空間：
     * 設定只記「專屬空間」不記路徑，換了位置會讓先前加入的文獻看不到。
     */
    private fun privateDir(): File? {
        val base = activity.getExternalFilesDir(null) ?: return null
        return File(base, "PaperReader")
    }

    @Command
    fun storageInfo(invoke: Invoke) {
        val privDir = privateDir()
        if (privDir == null) {
            invoke.reject("external files dir is not available")
            return
        }
        val ret = JSObject()
        ret.put("sdk", Build.VERSION.SDK_INT)
        ret.put("allFilesAccess", hasAllFiles())
        ret.put("sharedDir", sharedDir().absolutePath)
        ret.put("privateDir", privDir.absolutePath)
        ret.put("cacheDir", activity.cacheDir.absolutePath)
        invoke.resolve(ret)
    }

    @Command
    fun openAllFilesSettings(invoke: Invoke) {
        if (Build.VERSION.SDK_INT < 30) {
            invoke.reject("this Android version has no all-files-access page")
            return
        }
        val i = Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION)
        i.data = Uri.parse("package:" + activity.packageName)
        try {
            activity.startActivity(i)
        } catch (e: Exception) {
            activity.startActivity(Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION))
        }
        invoke.resolve()
    }

    // ---------- 把筆記檔交給別的應用 ----------

    /** 筆記檔放在文獻庫的 notes/；只交出共用空間（Documents/PaperReader）或應用專屬空間的 notes/ 底下的 .md 檔。 */
    @Command
    fun openExternal(invoke: Invoke) {
        val args = invoke.parseArgs(OpenExternalArgs::class.java)
        val file = File(args.path)
        // 只交出文獻庫的 notes/ 底下的 .md（對應 file_paths.xml 開放的兩處），以及匯出暫存資料夾裡的 .md
        // （重點筆記交出去的是一份唯讀的複本）
        val roots = listOfNotNull(
            File(sharedDir(), "notes"),
            privateDir()?.let { File(it, "notes") },
            File(activity.cacheDir, "exports"),
        ).map { it.canonicalPath + File.separator }
        if (!file.isFile || !file.name.endsWith(".md", ignoreCase = true) ||
            roots.none { file.canonicalPath.startsWith(it) }
        ) {
            val miss = JSObject()
            miss.put("result", "not_shareable")
            invoke.resolve(miss)
            return
        }
        val ret = JSObject()
        try {
            if (args.mode == "obsidian") {
                // 說明頁教使用者把保存庫設在文獻庫資料夾：保存庫名稱取文獻庫資料夾的名稱，檔案取從文獻庫算起的路徑
                // （notes/<檔名>）。不用 path=：Obsidian Android 收到絕對路徑不會開檔。
                val lib = file.canonicalFile.parentFile?.parentFile
                val rel = lib?.let { file.canonicalPath.removePrefix(it.canonicalPath + File.separator) } ?: file.name
                val link = Uri.parse(
                    "obsidian://open?vault=" + Uri.encode(lib?.name ?: "") + "&file=" + Uri.encode(rel)
                )
                activity.startActivity(Intent(Intent.ACTION_VIEW, link))
            } else {
                val uri = androidx.core.content.FileProvider.getUriForFile(
                    activity, activity.packageName + ".fileprovider", file
                )
                val view = Intent(Intent.ACTION_VIEW)
                view.setDataAndType(uri, "text/markdown")
                view.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
                activity.startActivity(Intent.createChooser(view, null))
            }
            ret.put("result", "opened")
        } catch (e: android.content.ActivityNotFoundException) {
            ret.put("result", "no_app")
        } catch (e: Exception) {
            invoke.reject("cannot open")
            return
        }
        invoke.resolve(ret)
    }

    // ---------- 用瀏覽器打開網頁 ----------

    /**
     * 「關於」頁的連結（CSL 專案、組件的原始碼位置）交給系統的瀏覽器打開。
     * 網頁元件裡直接點連結會讓應用自己的畫面換成那個網站，所以不用一般的連結。只接受 https。
     */
    @Command
    fun openUrl(invoke: Invoke) {
        val args = invoke.parseArgs(OpenUrlArgs::class.java)
        val uri = Uri.parse(args.url)
        if (uri.scheme != "https" || uri.host.isNullOrEmpty()) {
            invoke.reject("only https links")
            return
        }
        try {
            val view = Intent(Intent.ACTION_VIEW, uri)
            view.addCategory(Intent.CATEGORY_BROWSABLE)
            activity.startActivity(view)
            invoke.resolve()
        } catch (e: android.content.ActivityNotFoundException) {
            invoke.reject("no browser")
        }
    }

    // ---------- 閱讀時螢幕不關 ----------

    @Command
    fun setKeepScreenOn(invoke: Invoke) {
        val args = invoke.parseArgs(KeepScreenOnArgs::class.java)
        activity.runOnUiThread {
            if (args.on) {
                activity.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
            } else {
                activity.window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
            }
            invoke.resolve()
        }
    }

    // ---------- 系統選檔畫面 ----------

    @Command
    fun pickFiles(invoke: Invoke) {
        val args = invoke.parseArgs(PickArgs::class.java)
        val i = Intent(Intent.ACTION_OPEN_DOCUMENT)
        i.addCategory(Intent.CATEGORY_OPENABLE)
        i.type = if (args.kind == "pdf") "application/pdf" else "*/*"
        i.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, args.multiple)
        startActivityForResult(invoke, i, "pickFilesResult")
    }

    // ---------- 選資料夾（整批匯入） ----------

    @Command
    fun pickFolder(invoke: Invoke) {
        startActivityForResult(invoke, Intent(Intent.ACTION_OPEN_DOCUMENT_TREE), "pickFolderResult")
    }

    /**
     * 系統的資料夾選擇畫面交回 `content://` 樹狀位址。只收主要儲存空間（文件代號 `primary:<相對路徑>`），
     * 轉成檔案路徑交給外殼（應用有所有檔案存取權限時才讀得到）；記憶卡、雲端等其他提供者交回 `unsupported`。
     */
    @ActivityCallback
    fun pickFolderResult(invoke: Invoke, result: ActivityResult) {
        val ret = JSObject()
        val uri = result.data?.data
        if (result.resultCode != Activity.RESULT_OK || uri == null) {
            ret.put("cancelled", true)
            invoke.resolve(ret)
            return
        }
        ret.put("cancelled", false)
        val path = primaryStoragePath(uri)
        if (path == null) ret.put("unsupported", true) else ret.put("path", path)
        invoke.resolve(ret)
    }

    private fun primaryStoragePath(uri: Uri): String? {
        if (uri.authority != "com.android.externalstorage.documents") return null
        val id = try {
            DocumentsContract.getTreeDocumentId(uri)
        } catch (e: Exception) {
            return null
        }
        if (!id.startsWith("primary:")) return null
        val rel = id.removePrefix("primary:").trim('/')
        val root = Environment.getExternalStorageDirectory().absolutePath
        return if (rel.isEmpty()) root else "$root/$rel"
    }

    // ---------- 交出檔案：存到…、分享 ----------

    private var exportSource: File? = null

    @Command
    fun exportFile(invoke: Invoke) {
        val args = invoke.parseArgs(ExportArgs::class.java)
        val file = File(args.path)
        // 只交出應用暫存資料夾裡的檔
        if (!file.isFile || !file.canonicalPath.startsWith(activity.cacheDir.canonicalPath + File.separator)) {
            invoke.reject("not an exportable file")
            return
        }
        if (args.share) {
            try {
                val uri = androidx.core.content.FileProvider.getUriForFile(
                    activity, activity.packageName + ".fileprovider", file
                )
                val send = Intent(Intent.ACTION_SEND)
                send.type = args.mime ?: "application/zip"
                send.putExtra(Intent.EXTRA_STREAM, uri)
                send.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                activity.startActivity(Intent.createChooser(send, null))
                val ret = JSObject()
                ret.put("result", "shared")
                invoke.resolve(ret)
            } catch (e: Exception) {
                invoke.reject("cannot share")
            }
            return
        }
        exportSource = file
        val i = Intent(Intent.ACTION_CREATE_DOCUMENT)
        i.addCategory(Intent.CATEGORY_OPENABLE)
        i.type = args.mime ?: "application/zip"
        i.putExtra(Intent.EXTRA_TITLE, safeName(args.name))
        startActivityForResult(invoke, i, "exportFileResult")
    }

    @ActivityCallback
    fun exportFileResult(invoke: Invoke, result: ActivityResult) {
        val uri = result.data?.data
        val src = exportSource
        exportSource = null
        val ret = JSObject()
        if (result.resultCode != Activity.RESULT_OK || uri == null || src == null) {
            ret.put("result", "cancelled")
            invoke.resolve(ret)
            return
        }
        Thread {
            try {
                activity.contentResolver.openOutputStream(uri, "w")?.use { sink ->
                    src.inputStream().use { input -> input.copyTo(sink, 256 * 1024) }
                } ?: throw IllegalStateException("cannot open the target")
                ret.put("result", "saved")
                invoke.resolve(ret)
            } catch (e: Exception) {
                if (isNoSpace(e)) invoke.reject("no space", "NO_SPACE") else invoke.reject("cannot save")
            }
        }.start()
    }

    // ---------- 剪貼簿 ----------

    /** 放一段文字到剪貼簿；有 `html` 時一併放 HTML（貼到文書軟體保留斜體），純文字給不吃 HTML 的應用。 */
    @Command
    fun copyText(invoke: Invoke) {
        val args = invoke.parseArgs(ClipArgs::class.java)
        activity.runOnUiThread {
            try {
                val manager = activity.getSystemService(android.content.Context.CLIPBOARD_SERVICE) as ClipboardManager
                val html = args.html
                val clip = if (html.isNullOrEmpty()) {
                    ClipData.newPlainText("citations", args.text)
                } else {
                    ClipData.newHtmlText("citations", args.text, html)
                }
                manager.setPrimaryClip(clip)
                invoke.resolve()
            } catch (e: Exception) {
                invoke.reject("cannot copy")
            }
        }
    }

    // ---------- 列印：把一份 PDF 交給系統的列印服務 ----------

    /** 只印應用暫存資料夾裡的 PDF（原文的副本、帶標註的副本）。列印畫面開出來就回報，印不印由使用者決定。 */
    @Command
    fun printFile(invoke: Invoke) {
        val args = invoke.parseArgs(PrintArgs::class.java)
        val file = File(args.path)
        if (!file.isFile || !file.canonicalPath.startsWith(activity.cacheDir.canonicalPath + File.separator)) {
            invoke.reject("not a printable file")
            return
        }
        val jobName = safeName(args.name)
        activity.runOnUiThread {
            try {
                val manager = activity.getSystemService(android.content.Context.PRINT_SERVICE) as PrintManager
                manager.print(jobName, FilePrintAdapter(file, jobName), null)
                invoke.resolve()
            } catch (e: Exception) {
                invoke.reject("cannot print")
            }
        }
    }

    /** 把一份現成的 PDF 原樣交給列印服務（頁數交給服務自己數）。 */
    private class FilePrintAdapter(private val file: File, private val jobName: String) : PrintDocumentAdapter() {
        override fun onLayout(
            oldAttributes: PrintAttributes?,
            newAttributes: PrintAttributes?,
            cancellationSignal: CancellationSignal?,
            callback: LayoutResultCallback,
            extras: Bundle?,
        ) {
            if (cancellationSignal?.isCanceled == true) {
                callback.onLayoutCancelled()
                return
            }
            val info = PrintDocumentInfo.Builder(jobName)
                .setContentType(PrintDocumentInfo.CONTENT_TYPE_DOCUMENT)
                .setPageCount(PrintDocumentInfo.PAGE_COUNT_UNKNOWN)
                .build()
            callback.onLayoutFinished(info, true)
        }

        override fun onWrite(
            pages: Array<out PageRange>?,
            destination: ParcelFileDescriptor,
            cancellationSignal: CancellationSignal?,
            callback: WriteResultCallback,
        ) {
            // 複製檔案不佔主執行緒：在背景執行緒做，做完再回報
            Thread {
                try {
                    file.inputStream().use { input ->
                        java.io.FileOutputStream(destination.fileDescriptor).use { out ->
                            val buf = ByteArray(256 * 1024)
                            while (true) {
                                if (cancellationSignal?.isCanceled == true) {
                                    callback.onWriteCancelled()
                                    return@Thread
                                }
                                val n = input.read(buf)
                                if (n < 0) break
                                out.write(buf, 0, n)
                            }
                        }
                    }
                    callback.onWriteFinished(arrayOf(PageRange.ALL_PAGES))
                } catch (e: Exception) {
                    callback.onWriteFailed(e.message)
                }
            }.start()
        }
    }

    @ActivityCallback
    fun pickFilesResult(invoke: Invoke, result: ActivityResult) {
        val data = result.data
        val uris = ArrayList<Uri>()
        val clip = data?.clipData
        if (clip != null) {
            for (k in 0 until clip.itemCount) uris.add(clip.getItemAt(k).uri)
        } else if (data?.data != null) {
            uris.add(data.data!!)
        }
        if (result.resultCode != Activity.RESULT_OK || uris.isEmpty()) {
            val ret = JSObject()
            ret.put("cancelled", true)
            invoke.resolve(ret)
            return
        }
        // 每一份各自複製；一份失敗不影響其他份，也不留下那一份的暫存資料夾
        Thread {
            val files = JSArray()
            for (uri in uris) {
                files.put(copyOne(uri, File(activity.cacheDir, "picks")))
            }
            val ret = JSObject()
            ret.put("cancelled", false)
            ret.put("files", files)
            invoke.resolve(ret)
        }.start()
    }

    /**
     * 把一份選到（或分享進來）的檔案複製成暫存檔 `<root>/<隨機>/<檔名>`。
     * 失敗時只帶種類，不帶路徑與例外訊息。
     */
    private fun copyOne(uri: Uri, root: File): JSObject {
        val out = JSObject()
        var dir: File? = null
        var name = "chosen.pdf"
        try {
            name = safeName(displayName(uri))
            out.put("name", name)
            val made = File(root, UUID.randomUUID().toString())
            made.mkdirs()
            dir = made
            val dest = File(made, name)
            var bytes = 0L
            // 開啟與複製都在同一個 try 裡：任何一步失敗，這個暫存資料夾都會被刪掉
            val input = activity.contentResolver.openInputStream(uri)
                ?: throw IllegalStateException("cannot open the chosen file")
            input.use { src ->
                dest.outputStream().use { sink ->
                    val buf = ByteArray(256 * 1024)
                    while (true) {
                        val n = src.read(buf)
                        if (n < 0) break
                        sink.write(buf, 0, n)
                        bytes += n
                    }
                }
            }
            out.put("path", dest.absolutePath)
            out.put("bytes", bytes)
        } catch (e: Exception) {
            dir?.deleteRecursively()
            out.put("name", name)
            out.put("error", if (isNoSpace(e)) "no_space" else "failed")
        }
        return out
    }

    /** 寫入失敗是不是因為空間不足（ENOSPC：「No space left on device」）。 */
    private fun isNoSpace(e: Throwable): Boolean {
        var t: Throwable? = e
        while (t != null) {
            if (t is IOException && (t.message ?: "").contains("ENOSPC")) return true
            if (t is IOException && (t.message ?: "").contains("No space left", ignoreCase = true)) return true
            t = t.cause
        }
        return false
    }

    private fun displayName(uri: Uri): String {
        activity.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
            if (c.moveToFirst()) {
                val n = c.getString(0)
                if (!n.isNullOrEmpty()) return n
            }
        }
        return "chosen.pdf"
    }

    /** 顯示名稱可能含路徑分隔字元，當檔名之前先換掉。 */
    private fun safeName(name: String): String {
        val cleaned = name.replace('/', '_').replace('\\', '_').replace('\u0000', '_').trim()
        return if (cleaned.isEmpty() || cleaned == "." || cleaned == "..") "chosen.pdf" else cleaned
    }

    // ---------- 金鑰：用 Android Keystore 的 AES-GCM 金鑰加密，密文存在應用私有的 filesDir ----------
    //
    // 金鑰（API 金鑰）只在這裡加密、解密。網頁只會知道「有沒有設」；要送出翻譯請求時由 Rust 端來取。
    // 密文檔在 filesDir/secrets/，不在文獻庫裡，所以不進備份檔；清單設了 allowBackup=false，也不進系統備份。

    private val secretKeyAlias = "paperreader_secrets_v1"

    private fun secretFile(name: String): File? {
        if (!Regex("^[a-z0-9_]{1,40}$").matches(name)) return null
        return File(File(activity.filesDir, "secrets"), "$name.bin")
    }

    private fun secretKey(): SecretKey {
        val ks = KeyStore.getInstance("AndroidKeyStore")
        ks.load(null)
        (ks.getKey(secretKeyAlias, null) as? SecretKey)?.let { return it }
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        gen.init(
            KeyGenParameterSpec.Builder(
                secretKeyAlias,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build()
        )
        return gen.generateKey()
    }

    @Command
    fun secretSet(invoke: Invoke) {
        val args = invoke.parseArgs(SecretArgs::class.java)
        val file = secretFile(args.name)
        if (file == null || args.value.isEmpty()) {
            invoke.reject("invalid secret")
            return
        }
        try {
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.ENCRYPT_MODE, secretKey())
            val iv = cipher.iv
            val enc = cipher.doFinal(args.value.toByteArray(Charsets.UTF_8))
            file.parentFile?.mkdirs()
            val tmp = File(file.parentFile, file.name + ".tmp")
            tmp.writeBytes(byteArrayOf(iv.size.toByte()) + iv + enc)
            if (!tmp.renameTo(file)) {
                file.delete()
                if (!tmp.renameTo(file)) throw IOException("rename failed")
            }
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject("cannot store the secret")
        }
    }

    @Command
    fun secretGet(invoke: Invoke) {
        val args = invoke.parseArgs(SecretArgs::class.java)
        val file = secretFile(args.name)
        if (file == null) {
            invoke.reject("invalid secret")
            return
        }
        val ret = JSObject()
        if (!file.isFile) {
            // 沒有設過：不放 value 欄位（Rust 端讀成 None）
            invoke.resolve(ret)
            return
        }
        try {
            val all = file.readBytes()
            val ivLen = all[0].toInt()
            val iv = all.copyOfRange(1, 1 + ivLen)
            val enc = all.copyOfRange(1 + ivLen, all.size)
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, secretKey(), GCMParameterSpec(128, iv))
            ret.put("value", String(cipher.doFinal(enc), Charsets.UTF_8))
            invoke.resolve(ret)
        } catch (e: Exception) {
            invoke.reject("cannot read the secret")
        }
    }

    @Command
    fun secretDelete(invoke: Invoke) {
        val args = invoke.parseArgs(SecretArgs::class.java)
        val file = secretFile(args.name)
        if (file == null) {
            invoke.reject("invalid secret")
            return
        }
        file.delete()
        invoke.resolve()
    }

    /** 哪些名稱已經設了金鑰（只回名稱，不回內容）。 */
    @Command
    fun secretNames(invoke: Invoke) {
        val dir = File(activity.filesDir, "secrets")
        val arr = JSArray()
        dir.listFiles()?.filter { it.isFile && it.name.endsWith(".bin") }?.forEach {
            arr.put(it.name.removeSuffix(".bin"))
        }
        val ret = JSObject()
        ret.put("names", arr)
        invoke.resolve(ret)
    }

    // ---------- 網路 ----------

    /** 現在的網路是不是不計流量（Wi-Fi 之類）：「只在 Wi-Fi 下載」用。 */
    @Command
    fun networkInfo(invoke: Invoke) {
        val cm = activity.getSystemService(android.content.Context.CONNECTIVITY_SERVICE)
            as android.net.ConnectivityManager
        val caps = cm.getNetworkCapabilities(cm.activeNetwork)
        val ret = JSObject()
        ret.put("online", caps != null)
        ret.put(
            "unmetered",
            caps?.hasCapability(android.net.NetworkCapabilities.NET_CAPABILITY_NOT_METERED) == true,
        )
        invoke.resolve(ret)
    }

    // ---------- 應用內更新（做法在 AppUpdate.kt） ----------

    private val updateQueue = Executors.newSingleThreadExecutor()

    /** 系統記的這個應用的版本（不是編譯時寫進程式的）。 */
    @Command
    fun appVersion(invoke: Invoke) {
        val info = AppUpdate.installedInfo(activity)
        val ret = JSObject()
        ret.put("versionName", info.versionName ?: "")
        ret.put("versionCode", AppUpdate.versionCode(info))
        invoke.resolve(ret)
    }

    @Command
    fun inspectApk(invoke: Invoke) {
        val args = invoke.parseArgs(PathArgs::class.java)
        updateQueue.execute {
            try {
                val ret = JSObject()
                for ((k, v) in AppUpdate.inspect(activity, args.path)) ret.put(k, v)
                invoke.resolve(ret)
            } catch (e: Exception) {
                invoke.reject("cannot read apk")
            }
        }
    }

    /**
     * 還沒允許「安裝不明應用程式」時不開始，交回 needs_permission；寫進安裝 session 的內容和 `sha256` 不符時放棄，
     * 交回 mismatch；否則交給系統，交回 committed。
     */
    @Command
    fun installApk(invoke: Invoke) {
        val args = invoke.parseArgs(InstallArgs::class.java)
        if (!Regex("^[0-9a-fA-F]{64}$").matches(args.sha256)) {
            invoke.reject("missing sha256")
            return
        }
        val file = AppUpdate.inUpdates(activity, args.path)
        if (file == null) {
            invoke.reject("not a downloaded update")
            return
        }
        if (!AppUpdate.canInstall(activity)) {
            val ret = JSObject()
            ret.put("result", "needs_permission")
            invoke.resolve(ret)
            return
        }
        updateQueue.execute {
            try {
                val ok = AppUpdate.install(activity, file, args.sha256)
                val ret = JSObject()
                ret.put("result", if (ok) "committed" else "mismatch")
                invoke.resolve(ret)
            } catch (e: Exception) {
                invoke.reject("install failed")
            }
        }
    }

    @Command
    fun installStatus(invoke: Invoke) {
        val ret = JSObject()
        ret.put("status", AppUpdate.lastStatus)
        AppUpdate.lastStatus = null
        invoke.resolve(ret)
    }

    /** 系統的「安裝不明應用程式」頁，只有這個應用那一項。 */
    @Command
    fun openInstallSettings(invoke: Invoke) {
        if (Build.VERSION.SDK_INT < 26) {
            invoke.reject("this Android version has no per-app install permission")
            return
        }
        val i = Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES)
        i.data = Uri.parse("package:" + activity.packageName)
        try {
            activity.startActivity(i)
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject("cannot open settings")
        }
    }
}

@InvokeArg
class PathArgs {
    var path: String = ""
}

/** 安裝：下載檔的位置與核對過的 SHA-256（寫進安裝 session 時再算一次）。 */
@InvokeArg
class InstallArgs {
    var path: String = ""
    var sha256: String = ""
}
