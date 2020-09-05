package io.dragnea.git.rebaser

import org.eclipse.jgit.api.Git
import java.io.File

fun main() {
    println("hey")

    Git
        .open(File("/home/andrei/IdeaProjects/rebaser"))
        .branchList()
        .call()
        .forEach {
            println(it.name)
        }

}
