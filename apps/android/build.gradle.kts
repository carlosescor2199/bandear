plugins {
    id("com.android.application") version "8.7.3" apply false
    id("org.jetbrains.kotlin.android") version "2.0.21" apply false
    // Since Kotlin 2.0 the Compose compiler ships with Kotlin, so its version follows Kotlin's.
    id("org.jetbrains.kotlin.plugin.compose") version "2.0.21" apply false
}
