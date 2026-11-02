import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";

const platform = process.argv[2];
if (platform !== "ios" && platform !== "android") {
  console.error("Usage: bun apps/play/scripts/prepare-mobile.ts ios|android");
  process.exit(1);
}
const root = resolve(import.meta.dir, "../../..");
const icons = Bun.spawnSync(["bun", "run", "app:icons"], {
  cwd: root,
  stdout: "inherit",
  stderr: "inherit",
});
if (icons.exitCode !== 0) process.exit(icons.exitCode);
const result = Bun.spawnSync(
  ["bun", "run", "tauri", platform, "init", "--ci"],
  { cwd: root, stdout: "inherit", stderr: "inherit" },
);
if (result.exitCode !== 0) process.exit(result.exitCode);

if (platform === "android") {
  const activityPath = resolve(
    root,
    "src-tauri/gen/android/app/src/main/java/com/spellwake/client/MainActivity.kt",
  );
  const activity = await readFile(activityPath, "utf8");
  const shell = await readFile(
    resolve(root, "benchmarks/browser-rendering/public/play-area.html"),
    "utf8",
  );
  const ground = shell.match(/--ground:\s*(#[\da-f]{6})\s*;/i)?.[1];
  if (!ground) {
    console.error(
      "The native Android host requires the tabletop ground token.",
    );
    process.exit(1);
  }
  if (!activity.includes("WindowInsetsCompat.Type.systemBars()")) {
    const marker = "    super.onCreate(savedInstanceState)";
    if (activity.split(marker).length !== 2) {
      console.error("Android activity template changed; review system insets.");
      process.exit(1);
    }
    await writeFile(
      activityPath,
      activity
        .replace(
          "import android.os.Bundle",
          `import android.os.Bundle
import android.graphics.Color
import android.view.View
import androidx.core.graphics.Insets
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat`,
        )
        .replace(
          marker,
          `${marker}
    // Keep the WebView and touch targets outside system bars and display cutouts.
    val content = findViewById<View>(android.R.id.content)
    content.setBackgroundColor(Color.parseColor("${ground}"))
    WindowCompat.getInsetsController(window, content).apply {
      isAppearanceLightStatusBars = false
      isAppearanceLightNavigationBars = false
    }
    ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
      val types = WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
      val bars = insets.getInsets(types)
      view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
      WindowInsetsCompat.Builder(insets).setInsets(types, Insets.NONE).build()
    }
    ViewCompat.requestApplyInsets(content)`,
        ),
    );
  }
  const path = resolve(root, "src-tauri/gen/android/app/build.gradle.kts");
  const source = await readFile(path, "utf8");
  // The pinned Tauri template is generated, then this one signing seam is applied.
  const marker = "    buildTypes {";
  const signing = `    signingConfigs {
        create("release") {
            val keystorePath = System.getenv("ANDROID_KEYSTORE_PATH")
            if (keystorePath != null) {
                storeFile = file(keystorePath)
                storePassword = System.getenv("ANDROID_STORE_PASSWORD")
                keyAlias = System.getenv("ANDROID_KEY_ALIAS")
                keyPassword = System.getenv("ANDROID_KEY_PASSWORD")
            }
        }
    }

`;
  if (!source.includes('System.getenv("ANDROID_KEYSTORE_PATH")')) {
    if (source.split(marker).length !== 2) {
      console.error(
        "Android template changed; review the signing configuration.",
      );
      process.exit(1);
    }
    const release = 'getByName("release") {';
    if (source.split(release).length !== 2) {
      console.error(
        "Android release template changed; review signing configuration.",
      );
      process.exit(1);
    }
    await writeFile(
      path,
      source.replace(marker, signing + marker).replace(
        release,
        `${release}
            if (System.getenv("ANDROID_KEYSTORE_PATH") != null) {
                signingConfig = signingConfigs.getByName("release")
            }`,
      ),
    );
  }
} else {
  const project = await readFile(
    resolve(
      root,
      "src-tauri/gen/apple/spellwake-app.xcodeproj/project.pbxproj",
    ),
    "utf8",
  );
  if (!project.includes('TARGETED_DEVICE_FAMILY = "1,2"')) {
    console.error("The iOS project must support both iPhone and iPad.");
    process.exit(1);
  }
}
