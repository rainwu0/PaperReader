package io.github.rainwu0.paperreader.plugin

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInfo
import android.content.pm.PackageInstaller
import android.content.pm.PackageManager
import android.os.Build
import java.io.File
import java.security.MessageDigest

/**
 * 應用內更新在 Android 這一側做的事：讀下載的安裝檔（套件名稱、versionCode、簽章憑證）、
 * 交給系統的 `PackageInstaller` 安裝、接系統回報的結果。下載與 SHA-256 的核對在 Rust（`src-tauri/src/update.rs`）。
 *
 * 不用按「安裝」的條件（Android 12 起）：使用者允許過這個應用「安裝不明應用程式」、宣告
 * `UPDATE_PACKAGES_WITHOUT_USER_ACTION`、安裝 session 設 `USER_ACTION_NOT_REQUIRED`、更新的是自己。
 * 條件不合時系統回 `STATUS_PENDING_USER_ACTION`，由 [InstallStatusReceiver] 打開系統的確認畫面。
 */
object AppUpdate {
    /** 系統安裝程式最後回報的狀態（`PackageInstaller.STATUS_*`）；畫面取一次就清掉。 */
    @Volatile
    var lastStatus: Int? = null

    private fun hex(bytes: ByteArray): String =
        MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }

    fun versionCode(info: PackageInfo): Long =
        if (Build.VERSION.SDK_INT >= 28) info.longVersionCode else {
            @Suppress("DEPRECATION")
            info.versionCode.toLong()
        }

    private val signFlags: Int
        @Suppress("DEPRECATION")
        get() = if (Build.VERSION.SDK_INT >= 28) PackageManager.GET_SIGNING_CERTIFICATES else PackageManager.GET_SIGNATURES

    @Suppress("DEPRECATION")
    fun installedInfo(ctx: Context): PackageInfo =
        ctx.packageManager.getPackageInfo(ctx.packageName, signFlags)

    /**
     * 讀下載檔時兩個旗標一起給：Android 9、10 與 13 初版的 `getPackageArchiveInfo` 只在
     * `GET_SIGNATURES` 有設時才收集憑證，只給 `GET_SIGNING_CERTIFICATES` 時 `signingInfo` 是 null。
     * 兩個都設時，有收集憑證的版本照樣填 `signingInfo`（AOSP pie-release 的 `PackageParser.generatePackageInfo`）。
     */
    @Suppress("DEPRECATION")
    private val archiveFlags: Int
        get() = if (Build.VERSION.SDK_INT >= 28) {
            PackageManager.GET_SIGNING_CERTIFICATES or PackageManager.GET_SIGNATURES
        } else {
            PackageManager.GET_SIGNATURES
        }

    @Suppress("DEPRECATION")
    private fun archiveInfo(ctx: Context, path: String): PackageInfo? =
        ctx.packageManager.getPackageArchiveInfo(path, archiveFlags)

    /** 目前的簽章憑證（SHA-256）與輪替過的歷史；讀不到時 `null`（一律當成不符）。 */
    private class Signers(val current: Set<String>, val history: Set<String>, val multiple: Boolean)

    @Suppress("DEPRECATION")
    private fun signers(info: PackageInfo): Signers? {
        if (Build.VERSION.SDK_INT >= 28) {
            val si = info.signingInfo ?: return null
            val current = si.apkContentsSigners?.map { hex(it.toByteArray()) }?.toSet() ?: return null
            if (current.isEmpty()) return null
            val history = if (si.hasMultipleSigners()) emptySet()
            else si.signingCertificateHistory?.map { hex(it.toByteArray()) }?.toSet() ?: emptySet()
            return Signers(current, history, si.hasMultipleSigners())
        }
        val sigs = info.signatures ?: return null
        val current = sigs.map { hex(it.toByteArray()) }.toSet()
        if (current.isEmpty()) return null
        return Signers(current, emptySet(), sigs.size > 1)
    }

    /**
     * 下載的安裝檔和已安裝的是不是同一把金鑰簽的：目前的憑證完全相同；或者（單一金鑰時）下載的那份的輪替歷史裡
     * 有已安裝的那把（APK 簽章第三版的合法輪替，系統也接受）。讀不到任何一邊都算不同。
     */
    private fun sameSigner(a: Signers, b: Signers): Boolean {
        if (a.current == b.current) return true
        return !a.multiple && !b.multiple && a.current.size == 1 && b.history.containsAll(a.current)
    }

    /** 安裝檔必須在應用快取的 `updates/` 裡（Rust 下載到那裡）；別的位置一律不收。 */
    fun inUpdates(ctx: Context, path: String): File? {
        val root = File(ctx.cacheDir, "updates").canonicalPath + File.separator
        val f = File(path).canonicalFile
        return if (f.path.startsWith(root) && f.isFile && f.name.endsWith(".apk")) f else null
    }

    /**
     * 讀下載的安裝檔。`signerReadable` 為假表示讀不到任何一邊的簽章憑證（不是「不同」）；
     * 這時 `sameSigner` 也是假，一樣不裝。
     */
    fun inspect(ctx: Context, path: String): Map<String, Any> {
        val file = inUpdates(ctx, path)
        val archive = file?.let { archiveInfo(ctx, it.path) }
        if (archive == null) {
            return mapOf(
                "readable" to false,
                "samePackage" to false,
                "versionCode" to 0L,
                "signerReadable" to false,
                "sameSigner" to false,
            )
        }
        val mine = signers(installedInfo(ctx))
        val theirs = signers(archive)
        return mapOf(
            "readable" to true,
            "samePackage" to (archive.packageName == ctx.packageName),
            "versionCode" to versionCode(archive),
            "signerReadable" to (mine != null && theirs != null),
            "sameSigner" to (mine != null && theirs != null && sameSigner(mine, theirs)),
        )
    }

    /** 使用者允許過這個應用「安裝不明應用程式」。Android 8 以前沒有逐一應用的開關，由系統安裝程式自己問。 */
    fun canInstall(ctx: Context): Boolean =
        Build.VERSION.SDK_INT < 26 || ctx.packageManager.canRequestPackageInstalls()

    /**
     * 開一個安裝 session、寫入安裝檔、交給系統。結果由 [InstallStatusReceiver] 收。
     * 寫進安裝 session 時邊寫邊算 SHA-256：和 `sha256` 不符就放棄 session、不交給系統，交回假。
     * 這樣核對的就是系統要裝的那份內容，不是之前另外打開讀的那一次。
     */
    fun install(ctx: Context, file: File, sha256: String): Boolean {
        lastStatus = null
        val installer = ctx.packageManager.packageInstaller
        val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL)
        params.setAppPackageName(ctx.packageName)
        params.setSize(file.length())
        if (Build.VERSION.SDK_INT >= 26) params.setInstallReason(PackageManager.INSTALL_REASON_USER)
        if (Build.VERSION.SDK_INT >= 31) {
            params.setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_NOT_REQUIRED)
        }
        val id = installer.createSession(params)
        val session = installer.openSession(id)
        try {
            val md = MessageDigest.getInstance("SHA-256")
            file.inputStream().use { input ->
                session.openWrite("base.apk", 0, file.length()).use { out ->
                    val buf = ByteArray(1 shl 16)
                    while (true) {
                        val n = input.read(buf)
                        if (n < 0) break
                        md.update(buf, 0, n)
                        out.write(buf, 0, n)
                    }
                    session.fsync(out)
                }
            }
            val got = md.digest().joinToString("") { "%02x".format(it) }
            if (!got.equals(sha256, ignoreCase = true)) {
                session.abandon()
                return false
            }
            // 明確指定這個應用自己的接收者（不對外）；系統要在 intent 上填結果，所以 Android 12 起要可變的
            val intent = Intent(ctx, InstallStatusReceiver::class.java)
                .setAction(InstallStatusReceiver.ACTION)
                .setPackage(ctx.packageName)
            val flags = PendingIntent.FLAG_UPDATE_CURRENT or
                (if (Build.VERSION.SDK_INT >= 31) PendingIntent.FLAG_MUTABLE else 0)
            val pending = PendingIntent.getBroadcast(ctx, id, intent, flags)
            session.commit(pending.intentSender)
            session.close()
            return true
        } catch (e: Exception) {
            session.abandon()
            throw e
        }
    }
}

/**
 * 接系統安裝程式的回報（只收這個應用自己發的，manifest 裡 `exported="false"`）。
 * 要使用者確認時打開系統的確認畫面；其他結果記下來給畫面取。裝成功時這個應用會被系統結束，收不到成功。
 */
class InstallStatusReceiver : BroadcastReceiver() {
    companion object {
        const val ACTION = "io.github.rainwu0.paperreader.INSTALL_STATUS"
    }

    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != ACTION) return
        val status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)
        if (status == PackageInstaller.STATUS_PENDING_USER_ACTION) {
            val confirm: Intent? = if (Build.VERSION.SDK_INT >= 33) {
                intent.getParcelableExtra(Intent.EXTRA_INTENT, Intent::class.java)
            } else {
                @Suppress("DEPRECATION")
                intent.getParcelableExtra(Intent.EXTRA_INTENT)
            }
            if (confirm != null) {
                confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                try {
                    context.startActivity(confirm)
                } catch (e: Exception) {
                    AppUpdate.lastStatus = PackageInstaller.STATUS_FAILURE
                    return
                }
            }
        }
        AppUpdate.lastStatus = status
    }
}
