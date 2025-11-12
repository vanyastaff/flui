@echo off
set ANDROID_HOME=C:\Users\vanya\AppData\Local\Android\Sdk
set JAVA_HOME=C:\Users\vanya\AppData\Local\Programs\Android Studio\jbr

cd platforms\android
call gradlew.bat assembleDebug
