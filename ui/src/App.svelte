<script>
  import { onMount } from 'svelte'
  import Hls from 'hls.js'

  const API_BASE = import.meta.env.VITE_API_BASE_URL ?? ''
  const POLL_MS = 2000

  // --- State ---
  let ulid = null

  // Upload State
  let selectedFile = null
  let uploadStatus = '' 
  let uploadProgress = 0
  let uploadError = ''

  // Watch State
  let loading = true
  let error = ''
  let videoData = null
  
  // Player State
  let videoElement = null // Reference to the <video> DOM node
  let hlsInstance = null
  let activeUrl = null
  let isHlsActive = false

  // --- Router ---
  function parseWatchUlid(pathname) {
    const match = pathname.match(/^\/watch\/([^/]+)$/)
    return match ? decodeURIComponent(match[1]) : null
  }

  // --- Upload Flow ---
  async function handleUpload(e) {
    e.preventDefault()
    if (!selectedFile) return

    uploadStatus = 'requesting'
    uploadError = ''
    uploadProgress = 0

    try {
      const urlRes = await fetch(`${API_BASE}/api/upload-url`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          content_type: selectedFile.type || 'application/octet-stream',
          size_bytes: selectedFile.size
        })
      })

      if (!urlRes.ok) throw new Error(`Failed to get upload URL: ${await urlRes.text()}`)
      const { ulid: newUlid, upload_url, upload_complete_url } = await urlRes.json()

      uploadStatus = 'uploading'
      await new Promise((resolve, reject) => {
        const xhr = new XMLHttpRequest()
        xhr.open('PUT', upload_url)
        xhr.setRequestHeader('Content-Type', selectedFile.type || 'application/octet-stream')

        xhr.upload.onprogress = (event) => {
          if (event.lengthComputable) {
            uploadProgress = Math.round((event.loaded / event.total) * 100)
          }
        }

        xhr.onload = () => {
          if (xhr.status >= 200 && xhr.status < 300) resolve()
          else reject(new Error(`R2 rejected upload: ${xhr.status}`))
        }
        xhr.onerror = () => reject(new Error('Network error during R2 upload'))
        xhr.send(selectedFile)
      })

      uploadStatus = 'finalizing'
      const completeRes = await fetch(`${API_BASE}${upload_complete_url}`, { method: 'POST' })
      if (!completeRes.ok) throw new Error('Failed to notify backend of upload completion')

      uploadStatus = 'done'
      window.history.pushState({}, '', `/watch/${newUlid}`)
      currentPath = window.location.pathname
    } catch (err) {
      uploadStatus = 'error'
      uploadError = err.message
    }
  }

  // --- Watch Flow ---
  async function loadVideoMetadata() {
    if (!ulid) return

    try {
      const response = await fetch(`${API_BASE}/api/video/${ulid}`)
      if (!response.ok) throw new Error(`API returned ${response.status}`)
      
      const newVideoData = await response.json()
      videoData = newVideoData
      error = ''
    } catch (err) {
      error = err instanceof Error ? err.message : 'Failed to load video metadata'
    } finally {
      loading = false
    }
  }

// --- Player Orchestration ---
  $: if (videoElement && videoData) {
    updatePlayer(videoData)
  }

  function updatePlayer(data) {
    // 1. Phase 2: Check if HLS is fully ready
    if (data.status === 'ready' && data.manifest_url) {
      if (!isHlsActive) {
        initHls(data.manifest_url)
      }
      return
    }

    // 2. Phase 1: Fallback to Fast Paths (Raw or Transmuxed)
    let fallbackUrl = null
    
    if (data.transmux_url) {
        fallbackUrl = data.transmux_url
    } else if (data.browser_compatible && data.raw_url) {
        fallbackUrl = data.raw_url
    }

    // 3. Apply the fallback URL imperatively
    if (fallbackUrl && fallbackUrl !== activeUrl && !isHlsActive) {
      activeUrl = fallbackUrl
      
      if (videoElement) {
        // Save state in case we are upgrading from raw -> transmuxed mid-stream
        const currentTime = videoElement.currentTime || 0
        const wasPlaying = !videoElement.paused && currentTime > 0
        
        videoElement.src = fallbackUrl
        videoElement.load()
        
        // Restore state if necessary
        if (currentTime > 0) {
           videoElement.currentTime = currentTime
        }
        if (wasPlaying) {
           videoElement.play().catch(() => {})
        }
      }
    }
  }

  function initHls(manifestUrl) {
    if (!videoElement) return
    isHlsActive = true
    activeUrl = manifestUrl

    const currentTime = videoElement.currentTime || 0
    const wasPlaying = !videoElement.paused && currentTime > 0

    if (Hls.isSupported()) {
      if (hlsInstance) hlsInstance.destroy()
      
      hlsInstance = new Hls({ startPosition: currentTime })
      hlsInstance.loadSource(manifestUrl)
      hlsInstance.attachMedia(videoElement)
      
      hlsInstance.on(Hls.Events.MANIFEST_PARSED, () => {
        if (wasPlaying) videoElement.play().catch(() => {})
      })
    } 
    // Fallback for Safari (which supports HLS natively via the src attribute)
    else if (videoElement.canPlayType('application/vnd.apple.mpegurl')) {
      videoElement.src = manifestUrl
      videoElement.load() // <--- CRITICAL: Forces Safari to evaluate the manifest
      
      // Wait for metadata to load before attempting to set time or play
      videoElement.addEventListener('loadedmetadata', () => {
        videoElement.currentTime = currentTime
        if (wasPlaying) videoElement.play().catch(() => {})
      }, { once: true })
    }
  }

  function shouldStopPolling() {
    if (!videoData) return false
    // Terminal states: Stop hammering the API when it's fully HLS or permanently dead
    return videoData.status === 'ready' || videoData.status === 'failed'
  }

  function startWatchPolling() {
    loading = true
    let isUnmounted = false

    const poll = async () => {
      if (isUnmounted) return
      await loadVideoMetadata()
      if (!isUnmounted && !shouldStopPolling()) {
        setTimeout(poll, POLL_MS)
      }
    }
    poll()

    return () => { isUnmounted = true }
  }

// --- State Reset Helper ---
  function resetPlayerState() {
    if (hlsInstance) {
      hlsInstance.destroy()
      hlsInstance = null
    }
    videoData = null
    activeUrl = null
    isHlsActive = false
    loading = true
    error = ''
    
    if (videoElement) {
      videoElement.src = ''
      videoElement.removeAttribute('src')
      videoElement.load()
    }
  }

  // --- Lifecycle & Navigation Management ---
  let currentPath = window.location.pathname
  let stopPolling = () => {}

  // Listen for browser back/forward buttons
  window.addEventListener('popstate', () => {
    currentPath = window.location.pathname
  })

  $: {
    const newUlid = parseWatchUlid(currentPath)
    
    if (newUlid !== ulid) {
      ulid = newUlid
      stopPolling()
      resetPlayerState()

      if (ulid) {
        stopPolling = startWatchPolling()
      }
    }
  }
</script>

<main class="app-container">
  {#if !ulid}
    <section class="panel">
      <h1>Upload Video</h1>
      
      {#if uploadStatus === 'error'}
        <div class="error-box">{uploadError}</div>
      {/if}

      <form on:submit={handleUpload}>
        <input 
          type="file" 
          accept="video/*" 
          on:change={(e) => selectedFile = e.target.files[0]} 
          disabled={['requesting', 'uploading', 'finalizing'].includes(uploadStatus)}
        />
        <button 
          type="submit" 
          disabled={!selectedFile || ['requesting', 'uploading', 'finalizing'].includes(uploadStatus)}
        >
          Upload
        </button>
      </form>

      {#if uploadStatus === 'requesting'}
        <p>Negotiating upload slot...</p>
      {:else if uploadStatus === 'uploading'}
        <p>Uploading to R2: {uploadProgress}%</p>
        <div class="progress-bar"><div class="fill" style="width: {uploadProgress}%"></div></div>
      {:else if uploadStatus === 'finalizing'}
        <p>Probing media file...</p>
      {/if}
    </section>

  {:else}
    <!-- WATCH VIEW -->
    <section class="panel">
      <h1>Watch <code>{ulid}</code></h1>

      {#if loading}
        <p>Loading metadata...</p>
      {:else if error}
        <div class="error-box">{error}</div>
      {:else if videoData?.status === 'failed'}
        <div class="error-box">
          <h3>Processing Failed</h3>
          <p>We could not process this video. It may be corrupted or use an unsupported professional codec.</p>
        </div>
      {:else}
        
        <!-- The player is explicitly bound to the videoElement reference -->
        <div class="video-wrapper">
          <!-- svelte-ignore a11y_media_has_caption -->
          <video 
            bind:this={videoElement}
            class="player" 
            controls 
            playsinline
            crossorigin="anonymous"
            style={activeUrl ? 'opacity: 1;' : 'opacity: 0; pointer-events: none;'}>
          </video>
        </div>
        
        {#if !activeUrl}
          <div class="processing-placeholder">
            <div class="spinner"></div>
            <p>Video is processing. Please wait...</p>
          </div>
        {/if}

        <div class="status-indicator {videoData.status === 'ready' ? 'success' : 'warning'}">
          {#if videoData.status === 'ready'}
            Playing High Quality HLS Stream
          {:else if isHlsActive}
            Upgrading to HLS...
          {:else if videoData.browser_compatible}
            Fast path active: native playback from raw upload. Transcoding in background...
          {:else if videoData.transmux_required}
            Transmux active: playing repackaged MP4. Transcoding in background...
          {/if}
        </div>

        <div class="debug-card">
          <h4>Pipeline State</h4>
          <ul>
            <li>Status: <strong>{videoData.status}</strong></li>
            <li>Browser compatible: <strong>{String(videoData.browser_compatible)}</strong></li>
            <li>Transmux required: <strong>{String(videoData.transmux_required)}</strong></li>
            <li>HLS Active: <strong>{String(isHlsActive)}</strong></li>
          </ul>
        </div>

      {/if}
    </section>
  {/if}
</main>

<style>
  .app-container { max-width: 800px; margin: 2rem auto; font-family: system-ui, sans-serif; }
  .panel { border: 1px solid #ccc; padding: 2rem; border-radius: 8px; background: #fff; }
  .player { width: 100%; aspect-ratio: 16/9; background: #000; border-radius: 4px; }
  .video-wrapper { margin-bottom: 1rem; }
  .error-box { background: #fee; color: #c00; padding: 1rem; border-radius: 4px; margin-bottom: 1rem; border: 1px solid #fcc; }
  .progress-bar { width: 100%; background: #eee; height: 10px; border-radius: 5px; overflow: hidden; margin-top: 0.5rem; }
  .fill { background: #007bff; height: 100%; transition: width 0.2s; }
  .status-indicator { padding: 0.75rem; border-radius: 4px; margin-bottom: 1rem; font-weight: 500; }
  .success { background: #e6ffed; color: #006622; border: 1px solid #b3ffcc; }
  .warning { background: #fff8e6; color: #665500; border: 1px solid #ffeebb; }
  .debug-card { background: #f8f9fa; padding: 1rem; border-radius: 4px; font-size: 0.9em; }
  .processing-placeholder { aspect-ratio: 16/9; background: #f0f0f0; display: flex; flex-direction: column; align-items: center; justify-content: center; border-radius: 4px; color: #666; margin-bottom: 1rem; }
</style>
