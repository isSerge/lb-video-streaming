<script>
  import { onMount } from 'svelte'

  const API_BASE = import.meta.env.VITE_API_BASE_URL ?? ''
  const POLL_MS = 2000

  let ulid = null
  let loading = true
  let error = ''
  let video = null

  function parseWatchUlid(pathname) {
    const match = pathname.match(/^\/watch\/([^/]+)$/)
    return match ? decodeURIComponent(match[1]) : null
  }

  async function loadVideoMetadata() {
    if (!ulid) return

    try {
      const response = await fetch(`${API_BASE}/api/video/${ulid}`)
      if (!response.ok) {
        throw new Error(`API returned ${response.status}`)
      }
      video = await response.json()
      error = ''
    } catch (err) {
      error = err instanceof Error ? err.message : 'Failed to load video metadata'
    } finally {
      loading = false
    }
  }

  function isFastPathReady() {
    return Boolean(video?.status === 'uploaded' && video?.browser_compatible && video?.raw_url)
  }

  // Determine if we should stop hammering the API
  function shouldStopPolling() {
    if (!video) return false
    if (isFastPathReady()) return true
    if (video.status === 'failed') return true
    // When we implement HLS in Step 6, 'ready' will also be a terminal state
    if (video.status === 'ready') return true 
    return false
  }

  onMount(() => {
    ulid = parseWatchUlid(window.location.pathname)
    if (!ulid) {
      loading = false
      return
    }

    let isUnmounted = false;

    // Recursive polling function
    const poll = async () => {
      if (isUnmounted) return;
      
      await loadVideoMetadata();
      
      if (!isUnmounted && !shouldStopPolling()) {
        setTimeout(poll, POLL_MS);
      }
    };

    // Kick off the first request
    poll();

    // Cleanup on unmount
    return () => {
      isUnmounted = true;
    }
  })
</script>
