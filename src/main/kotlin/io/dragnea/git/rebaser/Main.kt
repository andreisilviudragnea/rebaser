package io.dragnea.git.rebaser

import org.eclipse.jgit.api.Git
import org.eclipse.jgit.api.RebaseCommand
import org.eclipse.jgit.api.RebaseResult
import org.eclipse.jgit.errors.RepositoryNotFoundException
import org.eclipse.jgit.transport.RefLeaseSpec
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

fun Git.isRebasedPrDirectlyOverMaster(pr: GHPullRequest): Boolean {
    val baseRef = pr.base.ref

    baseRef == "master" || return false

    val headRef = pr.head.ref

    val headCommit = repository.findRef(headRef).objectId
    val baseCommit = repository.findRef(baseRef).objectId

    return log().addRange(headCommit, baseCommit).call().toList().isEmpty()
}

fun Git.computeAllPrsToRebase(allPrs: List<GHPullRequest>): List<GHPullRequest> {
    return computeAllPrsToRebase(allPrs.filter { !isRebasedPrDirectlyOverMaster(it) }, "master")
}

fun Git.computeAllPrsToRebase(allPrs: List<GHPullRequest>, base: String): List<GHPullRequest> {
    val prsToRebase = allPrs.filter { it.base.ref == base && isSafePr(it) }

    val allPrsToRebase = prsToRebase.toMutableList()

    for (pr in prsToRebase) {
        allPrsToRebase += computeAllPrsToRebase(allPrs, pr.head.ref)
    }

    return allPrsToRebase
}

fun main() {
    val git = getRepository()

    git.fetch().call()

    val github = GitHubBuilder.fromPropertyFile().build()

    val myself = github.myself

    val gitRepository = git.repository

    val githubRepository = myself.getRepository(git.getRemoteName())

    val allPrs = githubRepository.getPullRequests(GHIssueState.OPEN).filter { it.user == myself }

    allPrs.forEach {
        val headRef = it.head.ref
        val baseRef = it.base.ref

        println("\"${it.title}\" $baseRef <- $headRef")

        val headCommit = gitRepository.findRef(headRef).objectId
        val baseCommit = gitRepository.findRef(baseRef).objectId

        val numberOfCommitsAhead = git.log().addRange(baseCommit, headCommit).call().toList().size
        val numberOfCommitsBehind = git.log().addRange(headCommit, baseCommit).call().toList().size

        println("\"$headRef\" is $numberOfCommitsAhead commits ahead, $numberOfCommitsBehind commits behind \"$baseRef\"")
    }

    val allPrsToRebase = git.computeAllPrsToRebase(allPrs)

    println("Going to rebase ${allPrsToRebase.size} pull requests in this order:")

    allPrsToRebase.forEach {
        println("\"${it.title}\" ${it.base.ref} <- ${it.head.ref}")
    }

    allPrsToRebase.forEach {
        val headRef = it.head.ref
        val baseRef = it.base.ref
        println("Rebasing \"${it.title}\" $baseRef <- $headRef...")

        val currentBranch = gitRepository.branch

        try {
            git.checkout().setName(headRef).call()

            val call = git.rebase().setUpstream(baseRef).call()

            if (call.status.isSuccessful) {
                if (git.isSafeBranch(headRef)) {
                    println("No changes for \"${it.title}\". Not pushing to remote.")
                    return@forEach
                }

                println("Successfully rebased \"${it.title}\". Pushing changes to remote...")

                git
                    .push()
                    .setRefLeaseSpecs(RefLeaseSpec("refs/heads/$headRef", "refs/origin/$headRef"))
                    .call()

                println("Successfully pushed changes to remote for \"${it.title}\".")

                return@forEach
            }

            println("Rebase error ${call.status} for \"${it.title}\". Aborting...")

            val abortResult = git.rebase().setOperation(RebaseCommand.Operation.ABORT).call()

            abortResult.status == RebaseResult.Status.ABORTED ||
                    throw IllegalStateException("Aborting rebase failed with status ${abortResult.status}")

            println("Successfully aborted \"${it.title}\".")
        } finally {
            git.checkout().setName(currentBranch).call()
        }

        println()
    }
}
