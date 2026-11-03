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
                "proguard-rules.pro"
            )
        }
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("src/main/jniLibs")
        }
    }
}

dependencies {
    // No dependencies needed - pure Rust/native app
}
