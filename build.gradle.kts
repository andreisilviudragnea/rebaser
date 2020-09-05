plugins {
    kotlin("jvm") version "1.4.0"
}

group = "io.dragnea"
version = "1.0-SNAPSHOT"

repositories {
    mavenCentral()
}

tasks {
    compileKotlin {
        kotlinOptions.jvmTarget = "1.8"
    }
    compileTestKotlin {
        kotlinOptions.jvmTarget = "1.8"
    }
}

dependencies {
    implementation(kotlin("stdlib"))
    implementation("org.eclipse.jgit:org.eclipse.jgit:5.8.1.202007141445-r")
    implementation("org.eclipse.jgit:org.eclipse.jgit.ssh.jsch:5.8.1.202007141445-r")
    implementation("org.kohsuke:github-api:1.116")
}
