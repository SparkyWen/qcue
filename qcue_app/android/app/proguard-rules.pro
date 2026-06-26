# QCue release (R8) keep rules.
#
# androidx.security:security-crypto pulls in Google Tink, which references
# compile-only annotations that are not present on the Android runtime
# classpath. Without these rules R8 aborts the release build with
# "Missing class com.google.errorprone.annotations.*" / "javax.annotation.*".
# They are annotations only — safe to not warn on.
-dontwarn com.google.errorprone.annotations.**
-dontwarn javax.annotation.**
-dontwarn javax.annotation.concurrent.**

# Tink uses reflection over its keyset/primitive classes; keep them intact.
-keep class com.google.crypto.tink.** { *; }

# Tink's optional remote-keyset downloader (KeysDownloader) references the Google
# HTTP client + Joda-Time, which we don't ship — that code path is never used.
-dontwarn com.google.api.client.**
-dontwarn com.google.api.**
-dontwarn org.joda.time.**

# Flutter's Play Store deferred-components / split-install integration references
# Play Core classes that are absent unless the Play Core lib is added. We don't
# use deferred components, so the referencing code never runs.
-dontwarn com.google.android.play.core.**
-keep class io.flutter.embedding.engine.deferredcomponents.** { *; }

# Flutter embedding + plugins (defensive; the Flutter plugin supplies most of
# these, but keeping them avoids reflective-lookup surprises under R8).
-keep class io.flutter.embedding.** { *; }

# androidx.work (WorkManager — the S5 background-flush scheduler) auto-initialises
# at process start via the androidx.startup InitializationProvider, BEFORE
# MainActivity. WorkManagerInitializer builds a Room database (WorkDatabase) whose
# generated *_Impl is instantiated REFLECTIVELY (Class.getDeclaredConstructor()
# .newInstance()). R8 full-mode strips that no-arg constructor because it sees no
# direct caller -> "NoSuchMethodException: WorkDatabase_Impl.<init> []" -> the
# provider fails -> the app crashes at launch (release-only; debug isn't minified).
# Keep the default constructor of every Room database (and WorkManager's by name).
-keep class * extends androidx.room.RoomDatabase { <init>(); }
-keep class androidx.work.impl.WorkDatabase_Impl { <init>(); }
# Room runtime references an optional Paging dependency we don't ship.
-dontwarn androidx.room.paging.**
