plugins {
    id("com.android.application")
}

// What is this machine's or this build's, `flui build` passes on the `gradlew`
// command line (`-Pflui.…`), never in gradle.properties, which is committed. A
// plain `./gradlew` runs without them: Gradle's own NDK, the debug key, and no
// Java sources or R8 rules of the linked crates.
fun fluiProperty(name: String): String = providers.gradleProperty(name).getOrElse("")

// The release key flui.toml names (`[signing.android]`); empty without one.
val releaseKeystore = fluiProperty("flui.signing.storeFile")

// A password variable set to an empty string counts as unset, as `flui build`
// counts it.
fun passwordVariable(name: String) =
    providers.environmentVariable(name).filter { it.isNotEmpty() }

android {
    // The package of the app's own Java or Kotlin code: it stays when flui.toml's
    // `[app] id` changes, as the code's package does.
    namespace = "{{package_name}}"
    // `flui build` writes both properties into gradle.properties.
    compileSdk = providers.gradleProperty("flui.targetSdk").get().toInt()
    // The NDK `cargo ndk` linked the libraries with, which `flui build` resolved:
    // Gradle strips them with the same one.
    fluiProperty("flui.ndkPath").takeIf { it.isNotEmpty() }?.let { ndkPath = it }

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
                keyAlias = fluiProperty("flui.signing.keyAlias")
                // The passwords come from the environment, never from a file.
                storePassword = passwordVariable("FLUI_ANDROID_KEYSTORE_PASSWORD").orNull
                keyPassword = passwordVariable("FLUI_ANDROID_KEY_PASSWORD")
                    .orElse(passwordVariable("FLUI_ANDROID_KEYSTORE_PASSWORD"))
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
                // The rules of the crates the app links, FLUI's own among them.
                *fluiProperty("flui.proguardFiles")
                    .split(File.pathSeparator)
                    .filter { it.isNotEmpty() }
                    .toTypedArray()
            )
        }
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("src/main/jniLibs")
            // The Java sources of the crates the app links, FluiActivity's among them.
            java.srcDirs(
                fluiProperty("flui.javaSources")
                    .split(File.pathSeparator)
                    .filter { it.isNotEmpty() }
            )
        }
    }
}

dependencies {
    // No dependencies needed - pure Rust/native app
}
