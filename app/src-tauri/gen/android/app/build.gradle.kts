import java.nio.file.Files
import java.nio.file.LinkOption
import java.util.Properties
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

// 正式版的簽章（Tauri 官方「Android Code Signing」的做法）：金鑰檔的位置、別名與密碼寫在
// gen/android/keystore.properties（被 .gitignore 擋，欄位見同一層的 keystore.properties.example）。
// 和官方範例不同的一點：沒有這個檔時不設簽章，而不是在設定階段就失敗，除錯版與其他人的建置照常；
// 這時正式版是未簽章的，`app/scripts/release-android.ps1` 會先檢查這個檔，缺了就停下說明。
val keystorePropertiesFile = rootProject.file("keystore.properties")
val keystoreProperties = Properties().apply {
    if (keystorePropertiesFile.exists()) {
        // 用 UTF-8 讀：金鑰檔的路徑裡有中文時才不會讀錯（Properties 預設用 ISO-8859-1）
        keystorePropertiesFile.reader(Charsets.UTF_8).use { load(it) }
    }
}

android {
    compileSdk = 37
    namespace = "io.github.rainwu0.paperreader"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "io.github.rainwu0.paperreader"
        minSdk = 24
        targetSdk = 37
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        // 對外的版號（規格/05 第 1、7 節）：Tauri 給的是設定檔的 `a.b.c+d`，系統設定裡要看到 `a.b.c.d`。
        // scripts/version-check.mjs 檢查這一行還在（Tauri 重新產生專案若蓋掉它，檢查會不過）。
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0").replace('+', '.')
    }
    if (keystorePropertiesFile.exists()) {
        signingConfigs {
            create("release") {
                fun need(key: String): String = keystoreProperties.getProperty(key)?.takeIf { it.isNotBlank() }
                    ?: throw GradleException("keystore.properties 缺少 $key（欄位見 keystore.properties.example）")
                keyAlias = need("keyAlias")
                keyPassword = need("password")
                storeFile = file(need("storeFile"))
                storePassword = need("password")
            }
        }
    }
    buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {
                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            if (keystorePropertiesFile.exists()) {
                signingConfig = signingConfigs.getByName("release")
            }
            optimization {
               enable = true
            }
            proguardFiles(
                *fileTree(".") {
                  include("**/*.pro")
                  exclude("build/**")
                }.files.toTypedArray()
            )
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
    buildFeatures {
        buildConfig = true
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_1_8
    }
}

rust {
    rootDirRel = "../../../"
}

// 版面模型（ONNX Runtime）在 Android 上要 libc++_shared.so。Tauri 的建置工具把 NDK 的原檔連結進
// jniLibs，原檔帶除錯資訊（x86_64 約 8.8 MB，剝掉後約 1.2 MB），Android 的建置工具也沒有替它剝。
// 打包前：應用的程式庫需要它，就換成剝過的複本（連結本身刪掉，不動 NDK 的原檔）；不需要（關掉功能、換了分支）
// 就刪掉留下的舊檔，免得照樣被打包。
val tidyCxxShared = tasks.register("tidyCxxShared") {
    val jniLibs = project.file("src/main/jniLibs")
    val windows = System.getProperty("os.name").lowercase().contains("windows")
    doLast {
        val ndk = System.getenv("NDK_HOME") ?: throw GradleException("NDK_HOME is not set")
        val bin = File(ndk, "toolchains/llvm/prebuilt").listFiles()?.firstOrNull { it.isDirectory }?.let { File(it, "bin") }
            ?: throw GradleException("no LLVM toolchain under $ndk")
        val exe = if (windows) ".exe" else ""
        fun tool(vararg cmd: String): String {
            val p = ProcessBuilder(*cmd).redirectErrorStream(true).start()
            val out = p.inputStream.bufferedReader().readText()
            if (p.waitFor() != 0) throw GradleException("${cmd.first()} failed: $out")
            return out
        }
        jniLibs.listFiles()?.filter { it.isDirectory }?.forEach { abi ->
            val cxx = File(abi, "libc++_shared.so").toPath()
            if (!Files.exists(cxx, LinkOption.NOFOLLOW_LINKS)) return@forEach
            val app = File(abi, "libpaperreader_app_lib.so")
            val needed = app.exists() && tool(File(bin, "llvm-readelf$exe").path, "--needed-libs", app.path).contains("libc++_shared.so")
            if (!needed) {
                Files.delete(cxx)
                logger.lifecycle("tidyCxxShared: removed unused ${abi.name}/libc++_shared.so")
                return@forEach
            }
            val stripped = File(abi, "libc++_shared.so.stripped")
            tool(File(bin, "llvm-strip$exe").path, "--strip-unneeded", "-o", stripped.path, cxx.toString())
            Files.delete(cxx)
            Files.move(stripped.toPath(), cxx)
            logger.lifecycle("tidyCxxShared: stripped ${abi.name}/libc++_shared.so (${Files.size(cxx)} bytes)")
        }
    }
}
tidyCxxShared.configure { mustRunAfter(tasks.matching { it.name.startsWith("rustBuild") }) }
tasks.matching { it.name.startsWith("merge") && it.name.endsWith("JniLibFolders") }.configureEach { dependsOn(tidyCxxShared) }

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = file("tauri.build.gradle.kts"))
