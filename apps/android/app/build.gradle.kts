import java.net.URI
import java.security.MessageDigest

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
}

// 仓库根（ime/）：Rust workspace 与随包数据都从这里取。
val repoRoot = rootDir.resolve("../..").canonicalFile
val nativeLibs = layout.buildDirectory.dir("native/jniLibs")
val sherpaLibs = layout.buildDirectory.dir("sherpa/jniLibs")
val bundledAssets = layout.buildDirectory.dir("bundled/assets")

// 版本号取 core/Cargo.toml；versionCode = 主*10000 + 次*100 + 修订，-dev 与正式版同号
val appVersion = rootDir.resolve("core/Cargo.toml").readLines()
    .first { it.startsWith("version = ") }
    .substringAfter('"').substringBefore('"')
val appVersionCode = appVersion.substringBefore('-').split('.').map(String::toInt)
    .let { (major, minor, patch) -> major * 10000 + minor * 100 + patch }

// 发版 CI 从 Secrets 注入，见 docs/notes/release.md
val releaseKeystore = System.getenv("QINGJIAN_ANDROID_KEYSTORE")?.takeIf { it.isNotEmpty() }

android {
    namespace = "app.qingjian.inputmethod"
    compileSdk = 36
    ndkVersion = "29.0.13599879"

    defaultConfig {
        applicationId = "com.mengren.shurufa"
        minSdk = 26
        targetSdk = 36
        versionCode = appVersionCode
        versionName = appVersion
        ndk { abiFilters += "arm64-v8a" }
    }

    if (releaseKeystore != null) {
        signingConfigs {
            create("release") {
                storeFile = file(releaseKeystore)
                storePassword = System.getenv("QINGJIAN_ANDROID_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("QINGJIAN_ANDROID_KEY_ALIAS")
                keyPassword = System.getenv("QINGJIAN_ANDROID_KEY_PASSWORD")
            }
        }
    }

    buildFeatures {
        buildConfig = true
    }

    buildTypes {
        debug {
            // 手机上装着发版签名的包时并排装调试版（-PsideBySide），不用卸载、不丢数据
            if (project.hasProperty("sideBySide")) applicationIdSuffix = ".dev"
        }
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName(if (releaseKeystore != null) "release" else "debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    sourceSets["main"].apply {
        jniLibs.srcDir(nativeLibs)
        jniLibs.srcDir(sherpaLibs)
        assets.srcDir(bundledAssets)
    }
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}

// Rust 侧：cargo ndk 编出 libqingjian_android.so，放进 jniLibs 源目录。要先 `cargo install cargo-ndk`。
val cargoNdk by tasks.registering(Exec::class) {
    group = "qingjian"
    description = "cargo ndk 编译 qingjian-android（arm64-v8a，release）"
    workingDir = repoRoot
    val ndkDir = android.sdkDirectory.resolve("ndk/${android.ndkVersion}")
    environment("ANDROID_NDK_HOME", ndkDir.absolutePath)
    commandLine(
        "cargo", "ndk",
        "-t", "arm64-v8a",
        "--platform", "26",
        "-o", nativeLibs.get().asFile.absolutePath,
        "build", "-p", "qingjian-android", "--release", "--locked",
    )
    // 增量交给 cargo 判断，这里每次都跑
    outputs.upToDateWhen { false }
}

// 随包数据：data/generated/ 的产品数据优先，没有就退回 assets/sample/ 样例（与 mac 的 bundle.sh 同一规矩）。
val stageData by tasks.registering(Sync::class) {
    group = "qingjian"
    description = "把随包数据放进 assets/data/"
    val generated = repoRoot.resolve("data/generated")
    val hasGenerated = generated.resolve("dict.qj").isFile
    into(bundledAssets.map { it.dir("data") })
    if (hasGenerated) {
        from(generated) {
            // 学习语言可选，几门语言的释义表都带上（共约 65 MB）
            include("dict.qj", "lm.qj", "glossary-*.qj", "english.tsv")
        }
        // 随包领域词库（与 PC 同一份），词库页里勾选
        from(generated.resolve("dicts")) { into("dicts") }
    } else {
        from(repoRoot.resolve("assets/sample")) { include("dict.tsv", "glossary-en.tsv", "english.tsv") }
    }
    from(repoRoot.resolve("assets/emoji")) { include("emoji-zh.tsv", "emoji-en.tsv") }
    // 词汇等级表（英语 CEFR、日语 JLPT），生词本按级筛与统计页按级分用
    from(repoRoot.resolve("assets/levels")) { include("levels-*.tsv") }
    // 五笔码表（形码方案用，与 PC 随包的同一份），解出来仍叫 wubi86.tsv
    from(repoRoot.resolve("assets/schemes/wubi86")) {
        include("table.tsv")
        rename { "wubi86.tsv" }
    }
}

// 本机离线语音识别的引擎（sherpa-onnx，Apache-2.0）：取 pub.dev 上 sherpa_onnx_android_arm64 包里预编译好的两个库，
// 版本必须与 crates/qingjian-voice-offline 的 EXPECTED_VERSION 及其结构体布局一致，换版本要一起改。
// 包缓存在 data/third_party/sherpa-onnx/（已忽略），只留当前版本；核对 SHA-256，不对就删了重下。
val sherpaVersion = "2.0.1"
val sherpaSha256 = "eceb1fd5de24d07c5aa618f6fa913f3f950018e6c009b899d076ba08209d08ec"
val sherpaArchive = repoRoot.resolve("data/third_party/sherpa-onnx/sherpa_onnx_android_arm64-$sherpaVersion.tar.gz")

fun sha256Of(file: File): String = MessageDigest.getInstance("SHA-256").let { digest ->
    file.inputStream().use { input ->
        val buffer = ByteArray(1 shl 16)
        while (true) {
            val read = input.read(buffer)
            if (read < 0) break
            digest.update(buffer, 0, read)
        }
    }
    digest.digest().joinToString("") { "%02x".format(it) }
}

val fetchSherpa by tasks.registering {
    group = "qingjian"
    description = "下载 sherpa-onnx 安卓库（pub.dev 包 sherpa_onnx_android_arm64 $sherpaVersion），校验 SHA-256"
    outputs.file(sherpaArchive)
    outputs.upToDateWhen { sherpaArchive.isFile && sha256Of(sherpaArchive) == sherpaSha256 }
    doLast {
        val folder = sherpaArchive.parentFile
        folder.mkdirs()
        // 旧版本的包不再用，删掉
        folder.listFiles()?.filter { it.name != sherpaArchive.name }?.forEach { it.delete() }
        val name = "sherpa_onnx_android_arm64-$sherpaVersion.tar.gz"
        val mirrors = listOf("https://pub.flutter-io.cn/api/archives/$name", "https://pub.dev/api/archives/$name")
        val partial = File(folder, "$name.part")
        val ok = mirrors.any { url ->
            runCatching {
                URI(url).toURL().openStream().use { input -> partial.outputStream().use { input.copyTo(it) } }
                sha256Of(partial) == sherpaSha256
            }.getOrDefault(false)
        }
        if (!ok) {
            partial.delete()
            throw GradleException("下载 sherpa-onnx 安卓库失败或校验不通过：$name")
        }
        sherpaArchive.delete()
        partial.renameTo(sherpaArchive)
    }
}

val stageSherpa by tasks.registering(Sync::class) {
    group = "qingjian"
    description = "把 sherpa-onnx 的 C 接口库与 onnxruntime 放进 jniLibs"
    dependsOn(fetchSherpa)
    from(tarTree(resources.gzip(sherpaArchive))) {
        include("android/src/main/jniLibs/arm64-v8a/libsherpa-onnx-c-api.so")
        include("android/src/main/jniLibs/arm64-v8a/libonnxruntime.so")
        eachFile { path = "arm64-v8a/$name" }
        includeEmptyDirs = false
    }
    into(sherpaLibs)
}

// 随包分发的第三方库的许可原文（sherpa-onnx 的 Apache-2.0 在同一个 pub 包里）
val stageLicenses by tasks.registering(Sync::class) {
    group = "qingjian"
    description = "把随包第三方库的许可原文放进 assets/licenses/"
    dependsOn(fetchSherpa)
    from(tarTree(resources.gzip(sherpaArchive))) {
        include("LICENSE")
        rename { "sherpa-onnx-LICENSE.txt" }
    }
    into(bundledAssets.map { it.dir("licenses") })
}

tasks.named("preBuild") {
    dependsOn(cargoNdk, stageData, stageSherpa, stageLicenses)
}

dependencies {
    testImplementation("junit:junit:4.13.2")
}
