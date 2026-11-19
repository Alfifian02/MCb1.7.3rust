package org.fossify.camera.helpers

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class CaptureSequencerTest {

    private class FakeScheduler : CaptureSequencer.Scheduler {
        val tasks = mutableListOf<Pair<Long, () -> Unit>>()
        override fun postDelayed(delayMs: Long, task: () -> Unit) {
            tasks.add(delayMs to task)
        }

        override fun cancelPending() = tasks.clear()

        fun runNext() = tasks.removeAt(0).second.invoke()
    }

    private class Recorder : CaptureSequencer.Listener {
        val progress = mutableListOf<Pair<Int, Int?>>()
        var finished: Pair<Int, Boolean>? = null
        override fun onProgress(taken: Int, total: Int?) {
            progress.add(taken to total)
        }

        override fun onFinished(taken: Int, completedAll: Boolean) {
            finished = taken to completedAll
        }
    }

    private val scheduler = FakeScheduler()
    private val recorder = Recorder()
    private var captures = 0
    private var succeed = true
    private val pendingCallbacks = mutableListOf<(Boolean) -> Unit>()
    private var autoComplete = true

    private val sequencer = CaptureSequencer(
        scheduler = scheduler,
        captureOne = { done ->
            captures++
            if (autoComplete) done(succeed) else pendingCallbacks.add(done)
        },
        listener = recorder
    )

    @Test
    fun burstTakesAllFramesBackToBackWithoutWaiting() {
        sequencer.startBurst(3)

        assertEquals(3, captures)
        assertTrue(scheduler.tasks.isEmpty())
        assertEquals(listOf(1 to 3, 2 to 3, 3 to 3), recorder.progress)
        assertEquals(3 to true, recorder.finished)
        assertFalse(sequencer.isRunning)
    }

    @Test
    fun timelapseWaitsForTheIntervalBetweenFrames() {
        sequencer.startTimelapse(2_000L)

        assertEquals(1, captures)
        assertEquals(2_000L, scheduler.tasks.single().first)
        assertTrue(sequencer.isRunning)

        scheduler.runNext()
        assertEquals(2, captures)
        assertEquals(listOf(1 to null, 2 to null), recorder.progress)
    }

    @Test
    fun stoppingCancelsTheNextFrame() {
        sequencer.startTimelapse(1_000L)
        sequencer.stop()

        assertTrue(scheduler.tasks.isEmpty())
        assertEquals(1 to false, recorder.finished)
        assertFalse(sequencer.isRunning)
    }

    @Test
    fun aFailedCaptureEndsTheSequence() {
        succeed = false
        sequencer.startBurst(5)

        assertEquals(1, captures)
        assertEquals(0 to false, recorder.finished)
    }

    @Test
    fun aLateCallbackOfAStoppedSequenceIsIgnored() {
        autoComplete = false
        sequencer.startBurst(3)
        sequencer.stop()
        pendingCallbacks.single().invoke(true)

        assertEquals(1, captures)
        assertTrue(recorder.progress.isEmpty())
    }

    @Test
    fun startingWhileRunningDoesNothing() {
        autoComplete = false
        sequencer.startBurst(3)
        sequencer.startBurst(3)

        assertEquals(1, captures)
    }
}
