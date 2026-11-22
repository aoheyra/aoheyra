# AND-22: the Gradle binding task generates UniFFI Kotlin into com.aoheyra.core
# (see crates/aoheyra/uniffi.toml). Generated classes are reached by reflection
# from native code and must survive R8 in a release build.
-keep class com.aoheyra.core.** { *; }
-keep interface com.aoheyra.core.** { *; }
-keep class * implements com.aoheyra.core.UniFFILib { *; }

# UniFFI 0.29 Kotlin bindings use JNA for the native library call.
-keep class com.sun.jna.** { *; }
-keepclasseswithmembers class * {
    native <methods>;
}

-keepattributes *Annotation*
-keepattributes Signature
-keepattributes Exceptions
-keepattributes InnerClasses
