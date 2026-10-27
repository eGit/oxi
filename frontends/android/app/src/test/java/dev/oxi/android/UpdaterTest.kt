package dev.oxi.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class UpdaterTest {
    @Test
    fun parsesReleaseTags() {
        assertEquals(Triple(1, 27, 0), Updater.parseVersion("v1.27.0"))
        assertEquals(Triple(1, 27, 3), Updater.parseVersion("1.27.3-beta+5"))
        assertNull(Updater.parseVersion("nightly"))
    }

    @Test
    fun onlyNewerVersionsAreUpdates() {
        assertTrue(Updater.isNewer("1.27.0", "1.26.9"))
        assertTrue(Updater.isNewer("2.0.0", "1.99.99"))
        assertFalse(Updater.isNewer("1.26.0", "1.26.0"))
        assertFalse(Updater.isNewer("1.25.9", "1.26.0"))
        assertFalse(Updater.isNewer("garbage", "1.26.0"))
    }
}
