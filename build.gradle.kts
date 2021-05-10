import org.jetbrains.kotlin.gradle.tasks.KotlinCompile

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
    withType<KotlinCompile> {
        kotlinOptions.jvmTarget = "16"
    }
    jar {
        manifest {
            attributes("Main-Class" to "io.dragnea.git.rebaser.MainKt")
        }

        from(configurations.compileClasspath.map { config -> config.map { if (it.isDirectory) it else zipTree(it) } })

        duplicatesStrategy = DuplicatesStrategy.WARN

        exclude("META-INF/*.RSA")
        exclude("META-INF/*.SF")
    }
}

dependencies {
    implementation(kotlin("stdlib"))
    implementation("org.eclipse.jgit:org.eclipse.jgit:5.11.0.202103091610-r")
    implementation("org.eclipse.jgit:org.eclipse.jgit.ssh.jsch:5.11.0.202103091610-r")
    implementation("org.kohsuke:github-api:1.128")
}
