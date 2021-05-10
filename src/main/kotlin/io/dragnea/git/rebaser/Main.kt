package io.dragnea.git.rebaser

import org.eclipse.jgit.api.Git
import org.eclipse.jgit.api.RebaseCommand
import org.eclipse.jgit.api.RebaseResult
import org.eclipse.jgit.api.ResetCommand
import org.eclipse.jgit.errors.RepositoryNotFoundException
import org.eclipse.jgit.transport.RefLeaseSpec
import org.eclipse.jgit.transport.RefSpec
import org.eclipse.jgit.transport.RemoteRefUpdate
import org.kohsuke.github.GHIssueState
import org.kohsuke.github.GHPullRequest
import org.kohsuke.github.GitHubBuilder
import java.nio.file.Path

fun getRepository(): Git {
    var toAbsolutePath = Path.of("").toAbsolutePath()

    while (true) {
        try {
            return Git.open(toAbsolutePath.toFile())
        } catch (e: RepositoryNotFoundException) {
            toAbsolutePath = toAbsolutePath.parent ?: throw e
        }
    }
}

fun Git.getRemoteName(): String {
    val remotes = remoteList().call()

    remotes.isNotEmpty() || throw IllegalStateException("Repository does not have any remote")

    if (remotes.size > 1) {
        println("Repository has more than one remote. Choosing the origin remote...")
    }

    val origin = remotes.first { it.name == "origin" }

    val uris = origin.urIs

    uris.isNotEmpty() || throw IllegalStateException("origin remote has no URI configured")

    if (uris.size > 1) {
        println("origin remote has more than one URI. Choosing the first one...")
    }

    val regexText = "\\S+/(\\S+)\\.git"

    val input = uris[0].path

    val matchResult = regexText.toRegex().matchEntire(input)
        ?: throw IllegalStateException("\"$input\" does not match \"$regexText\"")

    return matchResult.groupValues[1]
}

fun Git.isSafePr(pr: GHPullRequest): Boolean {
    val baseRef = pr.base.ref
    val safeBase = isSafeBranch(baseRef)

    if (!safeBase) {
        println("Pr \"${pr.title}\" is not safe because base ref \"$baseRef\" is not safe")
        return false
    }

    val headRef = pr.head.ref
    val safeHead = isSafeBranch(headRef)

    if (!safeHead) {
        println("Pr \"${pr.title}\" is not safe because head ref \"$headRef\" is not safe")
        return false
    }

    return true
}

fun Git.isSafeBranch(ref: String): Boolean {

    val commit = repository.findRef(ref).objectId
    val originRef = "origin/$ref"
    val originCommit = repository.findRef(originRef).objectId

    val n1 = log().addRange(commit, originCommit).call().toList().size

    if (n1 > 0) {
        println("Branch \"$ref\" is unsafe because it is $n1 commits behind \"$originRef\"")
        return false
    }

    val n2 = log().addRange(originCommit, commit).call().toList().size

    if (n2 > 0) {
        println("Branch \"$ref\" is unsafe because it is $n2 commits ahead \"$originRef\"")
        return false
    }

    return true
}

fun main() {
    val git = getRepository()

    git.fetch().call()

    val github = GitHubBuilder.fromPropertyFile().build()

    val myself = github.myself

    val remoteName = git.getRemoteName()

    val repository = myself.getRepository(remoteName)
        ?: throw IllegalArgumentException("Repository \"$remoteName\" not found in ${github.apiUrl}")

    println("Found repository \"${repository.fullName}\" in ${github.apiUrl}")

    val allPrs = repository
        .getPullRequests(GHIssueState.OPEN)
        .filter { it.user == myself }

    allPrs.forEach { it.describe(git) }

    val allPrsToRebase = allPrs.filter { git.isSafePr(it) }

    println()

    println("Going to rebase ${allPrsToRebase.size} safe pull requests:")

    allPrsToRebase.forEach {
        println("\"${it.title}\" ${it.base.ref} <- ${it.head.ref}")
    }

    println()

    do {
        var changesPropagated = false

        allPrsToRebase.forEach {
            changesPropagated = it.rebase(git) || changesPropagated
            println()
        }
    } while (changesPropagated)
}

private fun GHPullRequest.describe(git: Git) {
    val headRef = head.ref
    val baseRef = base.ref

    println("\"$title\" $baseRef <- $headRef")

    val headCommit = git.repository.findRef(headRef).objectId
    val baseCommit = git.repository.findRef(baseRef).objectId

    val numberOfCommitsAhead = git.log().addRange(baseCommit, headCommit).call().toList().size
    val numberOfCommitsBehind = git.log().addRange(headCommit, baseCommit).call().toList().size

    println("\"$headRef\" is $numberOfCommitsAhead commits ahead, $numberOfCommitsBehind commits behind \"$baseRef\"")
    println()
}

private fun GHPullRequest.rebase(git: Git): Boolean {
    val headRef = head.ref
    val baseRef = base.ref
    println("Rebasing \"$title\" $baseRef <- $headRef...")

    val currentBranch = git.repository.branch

    try {
        git.checkout().setName(headRef).call()

        val call = git.rebase().setUpstream(baseRef).call()

        if (call.status.isSuccessful) {
            if (git.isSafeBranch(headRef)) {
                println("No changes for \"$title\". Not pushing to remote.")
                return false
            }

            println("Successfully rebased \"$title\". Pushing changes to remote...")

            val pushResult = git
                .push()
                .setRefSpecs(RefSpec("refs/heads/$headRef").setForceUpdate(true))
                .setRefLeaseSpecs(RefLeaseSpec("refs/heads/$headRef", "refs/origin/$headRef"))
                .call()
                .map { it.remoteUpdates }
                .flatten()

            if (pushResult.any { it.status != RemoteRefUpdate.Status.OK }) {
                println("Push to remote failed for \"$title\": $pushResult. Resetting...")
                git.reset().setMode(ResetCommand.ResetType.HARD).setRef("origin/$headRef").call()
                return false
            }

            println("Successfully pushed changes to remote for \"$title\"")
            return true
        }

        println("Rebase error ${call.status} for \"$title\". Aborting...")

        val abortResult = git.rebase().setOperation(RebaseCommand.Operation.ABORT).call()

        abortResult.status == RebaseResult.Status.ABORTED ||
            throw IllegalStateException("Aborting rebase failed with status ${abortResult.status}")

        println("Successfully aborted \"$title\".")
    } finally {
        git.checkout().setName(currentBranch).call()
    }

    return false
}
