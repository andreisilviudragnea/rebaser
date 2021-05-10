plugins {
    kotlin("jvm") version "1.5.0"
    id("org.jlleitschuh.gradle.ktlint") version "10.0.0"
}

group = "io.dragnea"
version = "1.0-SNAPSHOT"

repositories {
    mavenCentral()
}

tasks {
    compileKotlin {
        kotlinOptions.jvmTarget = "16"
    }
    compileTestKotlin {
        kotlinOptions.jvmTarget = "16"
    }
}

dependencies {
    implementation(kotlin("stdlib"))
    implementation("org.eclipse.jgit:org.eclipse.jgit:5.11.0.202103091610-r")
    implementation("org.eclipse.jgit:org.eclipse.jgit.ssh.jsch:5.11.0.202103091610-r")
    implementation("org.kohsuke:github-api:1.128")
}
