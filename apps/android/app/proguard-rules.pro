# R8 rules for the BandEar app's release build.
#
# The app itself needs no keep rules: it uses no reflection, its JSON is read and written by
# explicit key names (org.json is part of the platform), and the ViewModel is built by an explicit
# factory. The libraries (Compose, AndroidX, OkHttp, Okio, coroutines) bring their own rules.

# Keep file names and line numbers, so a crash trace from a release build points at real lines.
-keepattributes SourceFile,LineNumberTable
-renamesourcefileattribute SourceFile
