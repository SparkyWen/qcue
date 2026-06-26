import java.io.FileInputStream
import java.util.Properties

plugins {
    id("com.android.application")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
}

// QCue release signing for Google Play. The real upload key + passwords live in
// android/key.properties (gitignored, NEVER committed — see key.properties.example).
// When that file is absent we fall back to the debug key below so `flutter run
// --release`, CI, and other devs without the keystore still build. A debug-signed
// AAB is REJECTED by the Play Console, so scripts/deploy-android.ps1 warns loudly
// when this fallback is the one in effect.
val keystoreProperties = Properties()
val keystorePropertiesFile = rootProject.file("key.properties")
val hasReleaseSigning = keystorePropertiesFile.exists()
if (hasReleaseSigning) {
    keystoreProperties.load(FileInputStream(keystorePropertiesFile))
}

android {
    namespace = "cn.qcue.app"
    compileSdk = flutter.compileSdkVersion
    ndkVersion = flutter.ndkVersion

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    defaultConfig {
        // TODO: Specify your own unique Application ID (https://developer.android.com/studio/build/application-id.html).
        applicationId = "cn.qcue.app"
        // You can update the following values to match your application needs.
        // For more information, see: https://flutter.dev/to/review-gradle-config.
        // NG-R14: Credential Manager's GetGoogleIdOption path (native Google sign-in) needs minSdk 23.
        minSdk = maxOf(23, flutter.minSdkVersion)
        targetSdk = flutter.targetSdkVersion
        versionCode = flutter.versionCode
        versionName = flutter.versionName
        // "Sign in with Google" (flutter_appauth). SECURITY (App Links): the ACTIVE OAuth redirect is now
        // the verified https App Link https://app.qcue.cn/applink/oauth2redirect (qcue_oidc.dart), caught
        // by the explicit RedirectUriReceiverActivity intent-filter in AndroidManifest.xml. This
        // placeholder stays as "qcue" ONLY because flutter_appauth's bundled manifest still references
        // ${appAuthRedirectScheme} on that activity (dropping it would fail the manifest merge); it keeps
        // the legacy qcue:// AppAuth receiver alive as a transition fallback. Remove this line (and the
        // qcue:// MainActivity filter) after on-device App Links verification passes — APP_LINKS.md step 5.
        manifestPlaceholders["appAuthRedirectScheme"] = "qcue"
    }

    signingConfigs {
        // Created ONLY when android/key.properties exists, so the debug-key fallback
        // in buildTypes.release stays valid on machines without the release keystore.
        if (hasReleaseSigning) {
            create("release") {
                keyAlias = keystoreProperties["keyAlias"] as String
                keyPassword = keystoreProperties["keyPassword"] as String
                storeFile = file(keystoreProperties["storeFile"] as String)
                storePassword = keystoreProperties["storePassword"] as String
                // Optional explicit keystore type. Unset → the JDK default (PKCS12 on
                // JDK 9+). Set `storeType=PKCS12` (our openssl-generated upload key) or
                // `storeType=JKS` (a legacy `keytool -storetype JKS` key) in
                // key.properties to load deterministically regardless of build-JDK default.
                (keystoreProperties["storeType"] as String?)?.let { storeType = it }
            }
        }
    }

    testOptions {
        // The native-plugin tests use Robolectric's framework but don't load app
        // resources/assets; keeping resources out avoids a Flutter asset-copy task
        // ordering conflict under Gradle 9.x. Default-return unmocked Android calls.
        unitTests.isReturnDefaultValues = true
    }

    buildTypes {
        release {
            // Play-uploadable upload key when android/key.properties is present;
            // otherwise the debug key so local `flutter run --release` still works.
            // deploy-android.ps1 warns when the debug fallback is in effect.
            signingConfig = if (hasReleaseSigning) {
                signingConfigs.getByName("release")
            } else {
                signingConfigs.getByName("debug")
            }
            // R8 shrink/minify with QCue keep rules (Tink/security-crypto needs
            // -dontwarn for its compile-only annotations — see proguard-rules.pro).
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}

dependencies {
    // QCue S5-R43/R44 — background outbox flush is a WorkManager PeriodicWork job
    // (BackgroundPlugin schedules it; FlushWorker runs it). androidx.core(-ktx)
    // is already on the classpath transitively via the Flutter embedding.
    implementation("androidx.work:work-runtime-ktx:2.10.0")
    // QCue S5-R24/R25/R26 — Keystore-backed secure key storage: a StrongBox-
    // preferring MasterKey + EncryptedSharedPreferences (SecurePlugin).
    implementation("androidx.security:security-crypto:1.1.0-alpha06")
    // QCue S5-R26 (D9) — the biometric gate (BiometricPrompt) that guards BYOK
    // vault reads; requires the FlutterFragmentActivity host (see MainActivity).
    implementation("androidx.biometric:biometric:1.1.0")

    // QCue — unit tests for the native plugins (Robolectric provides the Android
    // framework headlessly; mockito-kotlin for the MethodChannel.Result spies).
    // Robolectric runs against the pinned SDK in src/test/resources/robolectric.properties.
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.mockito.kotlin:mockito-kotlin:5.4.0")
    testImplementation("org.robolectric:robolectric:4.14.1")
    // Initialize WorkManager headlessly for the BackgroundPlugin scheduler tests.
    testImplementation("androidx.work:work-testing:2.10.0")
}

flutter {
    source = "../.."
}
