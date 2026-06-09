import { readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(scriptDir, "..", "..");
const gradleFile = resolve(
  repoRoot,
  "apps/mobile/src-tauri/gen/android/app/build.gradle.kts",
);

const signingValues = `/* cli-pocket android signing values start */
val androidSigningProperties = Properties().apply {
    val propFile = rootProject.file("keystore.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

val releaseStoreFile = providers.environmentVariable("ANDROID_KEYSTORE_PATH").orNull
    ?: androidSigningProperties.getProperty("storeFile")
val releaseStorePassword = providers.environmentVariable("ANDROID_KEYSTORE_PASSWORD").orNull
    ?: androidSigningProperties.getProperty("storePassword")
val releaseKeyAlias = providers.environmentVariable("ANDROID_KEY_ALIAS").orNull
    ?: androidSigningProperties.getProperty("keyAlias")
val releaseKeyPassword = providers.environmentVariable("ANDROID_KEY_PASSWORD").orNull
    ?: androidSigningProperties.getProperty("keyPassword")
val hasReleaseSigning = listOf(
    releaseStoreFile,
    releaseStorePassword,
    releaseKeyAlias,
    releaseKeyPassword,
).all { !it.isNullOrBlank() }
/* cli-pocket android signing values end */

`;

const signingConfig = `    /* cli-pocket android signing config start */
    if (hasReleaseSigning) {
        signingConfigs {
            create("release") {
                storeFile = rootProject.file(releaseStoreFile!!)
                storePassword = releaseStorePassword
                keyAlias = releaseKeyAlias
                keyPassword = releaseKeyPassword
            }
        }
    }
    /* cli-pocket android signing config end */
`;

const releaseBuildTypeSigning = `            if (hasReleaseSigning) {
                signingConfig = signingConfigs.getByName("release")
            }
`;

function replaceMarkedBlock(source, startMarker, endMarker, replacement) {
  const start = source.indexOf(startMarker);
  if (start === -1) {
    return null;
  }
  const end = source.indexOf(endMarker, start);
  if (end === -1) {
    throw new Error(`Found ${startMarker} without ${endMarker}`);
  }
  return `${source.slice(0, start)}${replacement}${source.slice(
    end + endMarker.length,
  )}`;
}

let source = readFileSync(gradleFile, "utf8");

const valuesStart = "/* cli-pocket android signing values start */";
const valuesEnd = "/* cli-pocket android signing values end */";
source =
  replaceMarkedBlock(source, valuesStart, valuesEnd, signingValues.trimEnd()) ??
  source.replace("android {\n", `${signingValues}android {\n`);

const configStart = "    /* cli-pocket android signing config start */";
const configEnd = "    /* cli-pocket android signing config end */";
source =
  replaceMarkedBlock(source, configStart, configEnd, signingConfig.trimEnd()) ??
  source.replace(
    '    namespace = "dev.cli_pocket.mobile"\n',
    `    namespace = "dev.cli_pocket.mobile"\n${signingConfig}`,
  );

if (!source.includes("signingConfig = signingConfigs.getByName(\"release\")")) {
  source = source.replace(
    '        getByName("release") {\n',
    `        getByName("release") {\n${releaseBuildTypeSigning}`,
  );
}

writeFileSync(gradleFile, source, "utf8");
console.log(`Configured Android release signing in ${gradleFile}`);
