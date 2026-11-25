// TabManager on Android: NativeActivity around libtabmanager.so,
// built from Rust via `cargo ndk` into src/main/jniLibs before APK packaging.

plugins {
    id("com.android.application")
}

val repo: File = rootDir.resolve("..").canonicalFile
val abis: List<String> = (findProperty("tabmanager.abis") as String? ?: "arm64-v8a")
    .split(",")
    .map { it.trim() }
    .filter { it.isNotEmpty() }
val cargoProfile: String = findProperty("tabmanager.cargoProfile") as String? ?: "release"

val versionMatch: MatchResult = Regex("""(?m)^version = "(\d+)\.(\d+)\.(\d+)"""")
    .find(repo.resolve("Cargo.toml").readText())
    ?: error("No version in ${repo.resolve("Cargo.toml")}")
val appVersionName: String = versionMatch.groupValues.drop(1).take(3).joinToString(".")
val versionParts: List<Int> = versionMatch.groupValues.drop(1).take(3).map { it.toInt() }
val appVersionCode: Int = versionParts[0] * 1_000_000 + versionParts[1] * 10_000 + versionParts[2] * 100

base {
    archivesName = "TabManager-$appVersionName"
}

android {
    namespace = "com.cyrexmv.tabmanager"
    compileSdk = 35
    ndkVersion = "29.0.14206865"

    defaultConfig {
        applicationId = "com.cyrexmv.tabmanager"
        minSdk = 26
        targetSdk = 35
        versionCode = appVersionCode
        versionName = appVersionName
        ndk {
            abiFilters += abis
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            // Signed with debug keystore for effortless sideloading on Galaxy Tab S12 Ultra
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    implementation("androidx.core:core:1.13.1")
}

val cargoExeName: String = if (System.getProperty("os.name").lowercase().contains("win")) "cargo.exe" else "cargo"
val cargo: String = listOfNotNull(
    System.getenv("CARGO_HOME"),
    "/opt/cargo",
    System.getProperty("user.home") + "/.cargo"
).map { File(it, "bin/$cargoExeName") }
    .firstOrNull { it.canExecute() }?.path ?: cargoExeName

val cargoNdk by tasks.registering(Exec::class) {
    description = "Builds libtabmanager.so for ${abis.joinToString()} with cargo-ndk."
    workingDir = repo
    val cargoBinDir = File(cargo).parent
    if (cargoBinDir != null) {
        environment("PATH", cargoBinDir + File.pathSeparator + System.getenv("PATH"))
    }
    val ndkDir = File(System.getenv("ANDROID_HOME") ?: "/opt/android-sdk", "ndk/29.0.14206865")
    if (ndkDir.exists()) {
        environment("ANDROID_NDK_HOME", ndkDir.absolutePath)
    }
    val args = mutableListOf(cargo, "ndk", "--platform", "26", "-o", file("src/main/jniLibs").path)
    abis.forEach { args += listOf("-t", it) }
    args += listOf("build", "--lib")
    if (cargoProfile == "release") args += "--release" else args += listOf("--profile", cargoProfile)
    commandLine(args)
}

tasks.named("preBuild") {
    dependsOn(cargoNdk)
}
