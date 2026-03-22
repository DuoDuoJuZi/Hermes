plugins {
    id("java")
    id("org.jetbrains.kotlin.jvm") version "1.9.0"
    id("org.jetbrains.intellij") version "1.17.4"
}

group = "com.duoduojuzi"
version = "0.1.0"

repositories {
    mavenCentral()
}

dependencies {
    implementation("org.java-websocket:Java-WebSocket:1.5.3") {
        exclude(group = "org.slf4j", module = "slf4j-api")
    }
}

intellij {
    version.set("2023.2.5")
    type.set("IC")
    plugins.set(listOf())
}

tasks {
    withType<JavaCompile> {
        sourceCompatibility = "17"
        targetCompatibility = "17"
    }
    withType<org.jetbrains.kotlin.gradle.tasks.KotlinCompile> {
        kotlinOptions.jvmTarget = "17"
    }
    patchPluginXml {
        sinceBuild.set("232")
        untilBuild.set("253.*")
    }
}

tasks.prepareSandbox {
    from("bin") {
        into("${intellij.pluginName.get()}/bin")
    }
}
