allprojects {
    repositories {
        google()
        mavenCentral()
    }
}

val newBuildDir: Directory =
    rootProject.layout.buildDirectory
        .dir("../../build")
        .get()
rootProject.layout.buildDirectory.value(newBuildDir)

subprojects {
    val newSubprojectBuildDir: Directory = newBuildDir.dir(project.name)
    project.layout.buildDirectory.value(newSubprojectBuildDir)
}
subprojects {
    project.evaluationDependsOn(":app")
}

// Align plugin (Android library) modules with the app's compileSdk. The pinned
// `sqlite3_flutter_libs` 0.5.27 declares compileSdk 32, but Flutter 3.44's
// embedding pulls androidx.fragment/window that require compileSdk >= 34, so the
// AAR-metadata check fails. Raise any library still below the app's level (36,
// FlutterExtension default) after it has been evaluated.
subprojects {
    val raiseCompileSdk = {
        val ext = extensions.findByName("android")
        if (ext is com.android.build.api.dsl.LibraryExtension && (ext.compileSdk ?: 0) < 36) {
            ext.compileSdk = 36
        }
    }
    // Some subprojects are already evaluated by the time this runs (the
    // evaluationDependsOn(":app") above forces it); afterEvaluate would throw on
    // those, so apply directly when already evaluated and defer otherwise.
    if (state.executed) raiseCompileSdk() else afterEvaluate { raiseCompileSdk() }
}

tasks.register<Delete>("clean") {
    delete(rootProject.layout.buildDirectory)
}
