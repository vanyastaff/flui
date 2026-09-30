allprojects {
    repositories {
        google()
        mavenCentral()
    }
}

// Gradle's default build directories stay in place: `flui build android`
// reads the APK from `app/build/outputs/apk/<profile>/`, and the scaffolded
// `.gitignore` and `flui clean` cover `build/` and `app/build/`.

tasks.register<Delete>("clean") {
    delete(rootProject.layout.buildDirectory)
}
