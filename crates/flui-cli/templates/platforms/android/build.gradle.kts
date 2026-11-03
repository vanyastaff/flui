plugins {
    id("com.android.application")
}

android {
    // The package of the app's own Java or Kotlin code: it stays when flui.toml's
    // `[app] id` changes, as the code's package does.
    namespace = "{{package_name}}"
    // `flui build` writes both properties into gradle.properties.
    compileSdk = providers.gradleProperty("flui.targetSdk").get().toInt()
    ndkVersion = "29.0.14206865"

    defaultConfig {
        // `flui build` writes it from flui.toml (`[app] id`) into gradle.properties.
        applicationId = providers.gradleProperty("flui.applicationId").get()
        minSdk = providers.gradleProperty("flui.minSdk").get().toInt()
        targetSdk = providers.gradleProperty("flui.targetSdk").get().toInt()
        versionCode = 1
        versionName = "1.0"
    }

    buildTypes {
        release {
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
