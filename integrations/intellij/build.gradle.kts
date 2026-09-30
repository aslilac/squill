import org.jetbrains.intellij.platform.gradle.IntelliJPlatformType
import org.jetbrains.intellij.platform.gradle.TestFrameworkType
import org.jetbrains.kotlin.gradle.dsl.KotlinVersion

plugins {
	kotlin("jvm") version "2.4.10"
	id("org.jetbrains.intellij.platform") version "2.19.0"
}

group = "dev.mckayla"
version = "0.1.0"

repositories {
	mavenCentral()
	intellijPlatform {
		defaultRepositories()
	}
}

dependencies {
	intellijPlatform {
		// The oldest platform with the open-source LSP client API. Built
		// against it, the plugin runs on every IDE since.
		intellijIdea("2026.1.4")
		testFramework(TestFrameworkType.Platform)
	}
	testImplementation("junit:junit:4.13.2")
	testImplementation("org.opentest4j:opentest4j:1.3.0")
}

tasks.test {
	// The squill the tests run: $SQUILL, else the workspace's debug build.
	environment(
		"SQUILL",
		System.getenv("SQUILL")
			?: rootDir.resolve("../../target/debug/squill").canonicalPath,
	)
	// Set to also test downloading squill's latest release from GitHub.
	System.getenv("SQUILL_TEST_DOWNLOAD")?.let { environment("SQUILL_TEST_DOWNLOAD", it) }
}

kotlin {
	jvmToolchain(21)
	compilerOptions {
		// The Kotlin that IDEs from 2026.1 bundle.
		apiVersion = KotlinVersion.KOTLIN_2_3
		languageVersion = KotlinVersion.KOTLIN_2_3
	}
}

intellijPlatform {
	pluginConfiguration {
		ideaVersion {
			sinceBuild = "261.26222"
			untilBuild = provider { null }
		}
	}
	// Nothing to index for Settings search beyond what plugin.xml names.
	buildSearchableOptions = false
	pluginVerification {
		ides {
			// The oldest supported platform, and the newest.
			create(IntelliJPlatformType.IntellijIdea, "2026.1.4")
			select {
				types = listOf(IntelliJPlatformType.IntellijIdea)
				sinceBuild = "262"
			}
		}
	}
}
