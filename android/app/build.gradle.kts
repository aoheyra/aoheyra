import java.io.File

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

// UniFFI reads proc-macro metadata from a host cdylib, so the Gradle task
// builds that library first and generates Kotlin from it. Both outputs stay
// under build/ (or the caller's CARGO_TARGET_DIR) and are never committed.
val rustRoot: File = rootProject.projectDir.parentFile
val cargoTargetDir: File = providers.environmentVariable("CARGO_TARGET_DIR")
    .orNull
    ?.let(::File)
    ?: layout.buildDirectory.dir("cargo-target").get().asFile
val hostLibraryName = when {
    System.getProperty("os.name").contains("Windows", ignoreCase = true) -> "aoheyra.dll"
    System.getProperty("os.name").contains("Mac", ignoreCase = true) -> "libaoheyra.dylib"
    else -> "libaoheyra.so"
}
val hostLibrary = File(cargoTargetDir, "debug/$hostLibraryName")
val uniffiConfig = File(rustRoot, "crates/aoheyra/uniffi.toml")
val generatedBindingsDir: File = layout.buildDirectory.dir("generated/source/uniffi/kotlin").get().asFile

val buildUniFFILibrary by tasks.registering(Exec::class) {
    group = "uniffi"
    description = "Builds the host cdylib that UniFFI reads metadata from."
    workingDir(rustRoot)
    environment("CARGO_TARGET_DIR", cargoTargetDir.absolutePath)
    inputs.files(
        File(rustRoot, "Cargo.toml"),
        File(rustRoot, "Cargo.lock"),
        File(rustRoot, "crates/aoheyra/Cargo.toml"),
    )
    inputs.dir(File(rustRoot, "crates/aoheyra/src"))
    outputs.file(hostLibrary)
    commandLine("cargo", "build", "--locked", "--lib")
}

val generateUniFFIBindings by tasks.registering(Exec::class) {
    group = "uniffi"
    description = "Generates Kotlin bindings for the aoheyra Rust bridge."
    dependsOn(buildUniFFILibrary)
    workingDir(rustRoot)
    environment("CARGO_TARGET_DIR", cargoTargetDir.absolutePath)
    inputs.file(hostLibrary)
    inputs.file(uniffiConfig)
    outputs.dir(generatedBindingsDir)
    commandLine(
        "cargo", "run", "--locked", "--features", "bindgen", "--bin", "uniffi-bindgen", "--",
        "generate",
        "--library", hostLibrary.absolutePath,
        "--language", "kotlin",
        "--config", uniffiConfig.absolutePath,
        "--out-dir", generatedBindingsDir.absolutePath,
        "--no-format",
    )
}

tasks.named("preBuild") { dependsOn(generateUniFFIBindings) }

android {
    namespace = "com.aoheyra.app"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.aoheyra.app"
        minSdk = 26
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"
    }

    // AND-22: signing material is supplied by the build environment only.
    // No keystore, alias or password is committed; build-release.sh checks
    // that every variable is present before a release task is invoked.
    signingConfigs {
        create("release") {
            System.getenv("ANDROID_KEYSTORE_PATH")?.let { storeFile = file(it) }
            storePassword = System.getenv("ANDROID_KEYSTORE_PASSWORD")
            keyAlias = System.getenv("ANDROID_KEY_ALIAS")
            keyPassword = System.getenv("ANDROID_KEY_PASSWORD")
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            signingConfig = signingConfigs.getByName("release")
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    // AND-22: one APK per ABI plus a universal APK for stores that need it.
    splits {
        abi {
            isEnable = true
            reset()
            include("arm64-v8a", "armeabi-v7a", "x86_64")
            isUniversalApk = true
        }
    }

    buildFeatures {
        compose = true
    }

    composeOptions {
        kotlinCompilerExtensionVersion = "1.5.14"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    sourceSets["main"].kotlin.srcDir(generatedBindingsDir)
}

dependencies {
    implementation(project(":ble"))
    implementation("androidx.core:core-ktx:1.13.1")

    // Required by the generated UniFFI Kotlin bindings (see the 0.29 docs).
    implementation("net.java.dev.jna:jna:5.13.0@aar")
    implementation("androidx.annotation:annotation:1.7.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.7.3")

    val composeBom = platform("androidx.compose:compose-bom:2024.04.00")
    implementation(composeBom)
    implementation("androidx.activity:activity-compose:1.9.0")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    debugImplementation("androidx.compose.ui:ui-tooling")

    testImplementation("junit:junit:4.13.2")
    androidTestImplementation(composeBom)
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
}
