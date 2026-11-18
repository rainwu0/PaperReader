package io.github.rainwu0.paperreader.plugin

import android.content.Context
import android.content.Intent
import android.content.pm.PackageInstaller
import android.os.Build
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

/**
 * 應用內更新失敗時留下系統的原話：狀態、`EXTRA_STATUS_MESSAGE`、舊式代碼、確認畫面屬於哪個套件、
 * 從交給系統與打開確認畫面到回報的時間，以及廠牌、系統版本、MIUI／HyperOS 的屬性、安裝者紀錄。
 *
 * 存在應用的私有空間一份小檔（`files/update-diagnostics.json`，最近 [KEEP] 次），不送到任何地方；畫面讀它顯示原因、
 * 讓使用者複製。寫檔而不是只放在記憶體：小米的確認畫面在前景時，系統可能結束 PaperReader 的程序，回報會送到一個新的程序。
 *
 * 檔的內容：`attempt` 是交給系統、還沒有最終結果的那一次（交給系統時寫，確認畫面打開時補上確認畫面的資訊）；
 * `reports` 是有了最終結果的幾次，最新的在最後。
 */
object UpdateDiagnostics {
    private const val FILE = "update-diagnostics.json"
    private const val KEEP = 5

    /** 隱藏的常數（`PackageInstaller.EXTRA_LEGACY_STATUS`），用字串讀。 */
    const val EXTRA_LEGACY_STATUS = "android.content.pm.extra.LEGACY_STATUS"

    private val lock = Any()

    private fun file(ctx: Context) = File(ctx.filesDir, FILE)

    private fun load(ctx: Context): JSONObject =
        try {
            JSONObject(file(ctx).readText())
        } catch (e: Exception) {
            JSONObject()
        }

    private fun save(ctx: Context, o: JSONObject) {
        val f = file(ctx)
        val tmp = File(f.path + ".tmp")
        tmp.writeText(o.toString())
        if (!tmp.renameTo(f)) {
            f.delete()
            tmp.renameTo(f)
        }
    }

    /** 交給系統之前：記下這一次（session、時間、要裝的 versionCode）。 */
    fun begin(ctx: Context, sessionId: Int, targetCode: Long) = synchronized(lock) {
        val o = load(ctx)
        o.put(
            "attempt",
            JSONObject()
                .put("sessionId", sessionId)
                .put("commitAt", System.currentTimeMillis())
                .put("targetCode", targetCode),
        )
        save(ctx, o)
    }

    /**
     * 系統要使用者確認：記下確認意圖指向哪裡、什麼時候打開。`resolveActivity` 在 Android 11 起的套件可見性下可能讀不到，
     * 所以意圖本身的套件、元件、動作也記。
     */
    fun confirming(ctx: Context, sessionId: Int, confirm: Intent) = synchronized(lock) {
        val o = load(ctx)
        val a = o.optJSONObject("attempt") ?: JSONObject().put("sessionId", sessionId)
        val resolved = try {
            confirm.resolveActivity(ctx.packageManager)?.flattenToShortString()
        } catch (e: Exception) {
            null
        }
        a.put("confirmAt", System.currentTimeMillis())
        a.put("confirmPackage", confirm.`package` ?: confirm.component?.packageName ?: resolved?.substringBefore('/'))
        a.put("confirmComponent", confirm.component?.flattenToShortString() ?: resolved)
        a.put("confirmAction", confirm.action)
        o.put("attempt", a)
        save(ctx, o)
    }

    /** 最終結果（成功或失敗）：和這一次的紀錄、環境資訊合成一筆，留最近幾筆。 */
    fun finish(ctx: Context, sessionId: Int, status: Int, message: String?, legacy: Int?) = synchronized(lock) {
        val o = load(ctx)
        val a = o.optJSONObject("attempt")
        val same = a != null && (sessionId < 0 || a.optInt("sessionId", -1) == sessionId)
        val r = JSONObject()
        if (same) {
            for (k in a!!.keys()) r.put(k, a.get(k))
            o.remove("attempt")
        } else {
            r.put("sessionId", sessionId)
        }
        r.put("at", System.currentTimeMillis())
        r.put("status", status)
        r.put("statusMessage", message)
        if (legacy != null) r.put("legacyStatus", legacy)
        environment(ctx, r)
        val list = o.optJSONArray("reports") ?: JSONArray()
        list.put(r)
        val kept = JSONArray()
        for (i in maxOf(0, list.length() - KEEP) until list.length()) kept.put(list.get(i))
        o.put("reports", kept)
        save(ctx, o)
    }

    /**
     * 最新的一筆最終結果，以及還沒有結果的那一次（連同現在的環境資訊、狀態記成 `STATUS_PENDING_USER_ACTION`）。
     * 系統一直沒有回報時（確認畫面一閃就不見），畫面用後者說明並讓使用者複製。
     */
    fun latest(ctx: Context): Pair<JSONObject?, JSONObject?> = synchronized(lock) {
        val o = load(ctx)
        val list = o.optJSONArray("reports")
        val last = if (list != null && list.length() > 0) list.optJSONObject(list.length() - 1) else null
        val pending = o.optJSONObject("attempt")?.let { a ->
            val r = JSONObject()
            for (k in a.keys()) r.put(k, a.get(k))
            r.put("at", System.currentTimeMillis())
            r.put("status", PackageInstaller.STATUS_PENDING_USER_ACTION)
            environment(ctx, r)
            r
        }
        Pair(last, pending)
    }

    /** 已安裝的版本、安裝者紀錄、是否允許安裝不明應用程式、裝置與系統。 */
    private fun environment(ctx: Context, r: JSONObject) {
        try {
            val info = AppUpdate.installedInfo(ctx)
            r.put("installedName", info.versionName ?: "")
            r.put("installedCode", AppUpdate.versionCode(info))
        } catch (e: Exception) {
            // 讀不到就不記
        }
        try {
            if (Build.VERSION.SDK_INT >= 30) {
                val src = ctx.packageManager.getInstallSourceInfo(ctx.packageName)
                r.put("installer", src.installingPackageName)
                r.put("initiator", src.initiatingPackageName)
            } else {
                @Suppress("DEPRECATION")
                r.put("installer", ctx.packageManager.getInstallerPackageName(ctx.packageName))
            }
        } catch (e: Exception) {
            // 讀不到就不記
        }
        r.put("canRequestInstalls", AppUpdate.canInstall(ctx))
        r.put("device", device())
    }

    /** 廠牌、型號、系統版本、MIUI／HyperOS 的屬性（讀不到的欄位不放）。不含任何個人資料。 */
    fun device(): JSONObject {
        val d = JSONObject()
        d.put("manufacturer", Build.MANUFACTURER)
        d.put("brand", Build.BRAND)
        d.put("model", Build.MODEL)
        d.put("display", Build.DISPLAY)
        d.put("release", Build.VERSION.RELEASE)
        d.put("sdk", Build.VERSION.SDK_INT)
        d.put("incremental", Build.VERSION.INCREMENTAL)
        d.put("miuiVersion", prop("ro.miui.ui.version.name"))
        d.put("hyperOsVersion", prop("ro.mi.os.version.name"))
        d.put("miuiOptimization", prop("persist.sys.miui_optimization"))
        return d
    }

    /**
     * 讀一個系統屬性：先用 `android.os.SystemProperties.get`（反射，Droid-ify、Aurora Store 的做法），不行再跑 `getprop`。
     * 都讀不到交回 null（記成「未知」）；屬性沒設時是空字串。
     */
    private fun prop(name: String): String? {
        try {
            val c = Class.forName("android.os.SystemProperties")
            return c.getMethod("get", String::class.java).invoke(null, name) as String
        } catch (e: Throwable) {
            // 換下一種
        }
        return try {
            val p = ProcessBuilder("getprop", name).redirectErrorStream(true).start()
            val out = p.inputStream.bufferedReader().readText().trim()
            if (p.waitFor() == 0) out else null
        } catch (e: Throwable) {
            null
        }
    }

    /** 收到系統回報時的處理（[InstallStatusReceiver] 呼叫）。 */
    fun onStatus(ctx: Context, intent: Intent, status: Int) {
        val sessionId = intent.getIntExtra(PackageInstaller.EXTRA_SESSION_ID, -1)
        val message = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE)
        val legacy = if (intent.hasExtra(EXTRA_LEGACY_STATUS)) intent.getIntExtra(EXTRA_LEGACY_STATUS, 0) else null
        finish(ctx, sessionId, status, message, legacy)
    }
}
