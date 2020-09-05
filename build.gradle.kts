plugins {
    kotlin("jvm") version "1.4.0"
}

group = "io.dragnea"
version = "1.0-SNAPSHOT"

repositories {
    mavenCentral()
}

dependencies {
    implementation(kotlin("stdlib"))
    implementation("org.eclipse.jgit:org.eclipse.jgit:5.8.1.202007141445-r")
}
