plugins {
    id("com.android.application")
}

// The release key `flui build` writes from flui.toml (`[signing.android]`) into
// gradle.properties; empty when the application names none.
val releaseKeystore = providers.gradleProperty("flui.signing.storeFile").get()

android {
    // The package of the app's own Java or Kotlin code: it stays when flui.toml's
    // `[app] id` changes, as the code's package does.
    namespace = "{{package_name}}"
    // `flui build` writes both properties into gradle.properties.
    compileSdk = providers.gradleProperty("flui.targetSdk").get().toInt()
    // The NDK `cargo ndk` linked the libraries with, which `flui build` resolved
    // and writes into gradle.properties: Gradle strips them with the same one.
    ndkPath = providers.gradleProperty("flui.ndkPath").get()

    defaultConfig {
        // `flui build` writes it from flui.toml (`[app] id`) into gradle.properties.
        applicationId = providers.gradleProperty("flui.applicationId").get()
        minSdk = providers.gradleProperty("flui.minSdk").get().toInt()
        targetSdk = providers.gradleProperty("flui.targetSdk").get().toInt()
        // `flui build` writes both from flui.toml (`[app] version`) into
        // gradle.properties: versionCode is major * 10000 + minor * 100 + patch.
        versionCode = providers.gradleProperty("flui.versionCode").get().toInt()
        versionName = providers.gradleProperty("flui.versionName").get()
    }

    signingConfigs {
        create("release") {
            if (releaseKeystore.isNotEmpty()) {
                storeFile = file(releaseKeystore)
                keyAlias = providers.gradleProperty("flui.signing.keyAlias").get()
                // The passwords come from the environment, never from a file.
                storePassword = providers.environmentVariable("FLUI_ANDROID_KEYSTORE_PASSWORD").orNull
                keyPassword = providers.environmentVariable("FLUI_ANDROID_KEY_PASSWORD")
                    .orElse(providers.environmentVariable("FLUI_ANDROID_KEYSTORE_PASSWORD"))
                    .orNull
            }
        }
    }

    buildTypes {
        release {
            // Without a release key the release is signed with the debug key, which
            // `flui build` says; a store refuses such an APK.
            signingConfig = if (releaseKeystore.isNotEmpty()) {
                signingConfigs.getByName("release")
            } else {
                signingConfigs.getByName("debug")
            }
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
                // The rules of the crates the app links, FLUI's own among them: `flui build`
                // writes their paths into gradle.properties.
                *providers.gradleProperty("flui.proguardFiles").get()
                    .split(File.pathSeparator)
                    .filter { it.isNotEmpty() }
                    .toTypedArray()
            )
        }
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("src/main/jniLibs")
            // The Java sources of the crates the app links, FluiActivity's among
            // them: `flui build` writes their directories into gradle.properties.
            java.srcDirs(
                providers.gradleProperty("flui.javaSources").get()
                    .split(File.pathSeparator)
                    .filter { it.isNotEmpty() }
            )
        }
    }
}

dependencies {
    // No dependencies needed - pure Rust/native app
}
