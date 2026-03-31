<script>
  import { onMount } from 'svelte'

  const API_BASE = import.meta.env.VITE_API_BASE_URL ?? ''
  const POLL_MS = 2000

  // --- State ---
  let ulid = null

  // Upload State
  let selectedFile = null
  let uploadStatus = '' // 'idle' | 'requesting' | 'uploading' | 'finalizing' | 'done' | 'error'
  let uploadProgress = 0
  let uploadError = ''

  // Watch State
  let loading = true
  let error = ''
  let video = null

  let pollingTimeout = null

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
        xhr.onerror = () => reject(new Error('Network error during R2 upload (Check CORS rules)'))
        xhr.send(selectedFile)
      })

      uploadStatus = 'finalizing'
      const completeRes = await fetch(`${API_BASE}${upload_complete_url}`, { method: 'POST' })
      if (!completeRes.ok) throw new Error('Failed to notify backend of upload completion')

      uploadStatus = 'done'
      window.history.pushState({}, '', `/watch/${newUlid}`)
      ulid = newUlid
      startWatchPolling()

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
      video = await response.json()
      error = ''
    } catch (err) {
      error = err instanceof Error ? err.message : 'Failed to load video metadata'
    } finally {
      loading = false
    }
  }

  function getPlayableUrl() {
    if (!video) return null
    // Phase 1a: Instant raw if browser compatible
    if (video.browser_compatible) return video.raw_url
    // Phase 1b: Transmuxed source if available (status may be 'uploaded' or 'transcoding')
    if (video.transmux_url) return video.transmux_url
    return null
  }

  function shouldStopPolling() {
    if (!video) return false
    if (video.status === 'failed') return true
    if (video.status === 'ready') return true
    return false
  }

  function startWatchPolling() {
    if (pollingTimeout) clearTimeout(pollingTimeout)

    loading = true
    let isCancelled = false

    const poll = async () => {
      if (isCancelled) return
      await loadVideoMetadata()
      if (!isCancelled && !shouldStopPolling()) {
        pollingTimeout = setTimeout(poll, POLL_MS)
      }
    }
    poll()

    // Return cleanup function
    return () => {
      isCancelled = true
      if (pollingTimeout) clearTimeout(pollingTimeout)
      pollingTimeout = null
    }
  }

  // --- Lifecycle ---
  let cleanupPolling = null

  onMount(() => {
    ulid = parseWatchUlid(window.location.pathname)
    if (ulid) {
      cleanupPolling = startWatchPolling()
    }
    return () => {
      if (cleanupPolling) cleanupPolling()
    }
  })

  window.addEventListener('popstate', () => {
    if (cleanupPolling) cleanupPolling()
    ulid = parseWatchUlid(window.location.pathname)
    if (ulid) {
      cleanupPolling = startWatchPolling()
    }
  })
</script>

<main class="app-container">
  {#if !ulid}
    <!-- UPLOAD VIEW -->
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
      {:else}
        
        {#if getPlayableUrl()}
          <!-- svelte-ignore a11y_media_has_caption -->
          <video class="player" controls playsinline src={getPlayableUrl()}></video>
          
          <div class="status-indicator success">
            {#if video.browser_compatible}
              Fast path active: native playback from raw upload.
            {:else if video.transmux_url}
              Transmux active: playing repackaged MP4/WebM. HLS will take over when ready.
            {:else}
              Processing: video will play as soon as repackaging is complete.
            {/if}
          </div>
        {:else}
          <div class="processing-placeholder">
            <div class="spinner"></div>
            <p>Video is processing. Please wait...</p>
          </div>
        {/if}

        <div class="debug-card">
          <h4>Pipeline State</h4>
          <ul>
            <li>Status: <strong>{video.status}</strong></li>
            <li>Browser compatible: <strong>{String(video.browser_compatible)}</strong></li>
            <li>Transmux required: <strong>{String(video.transmux_required)}</strong></li>
            <li>Transmux URL: <strong>{video.transmux_url ? '✅' : '❌'}</strong></li>
          </ul>
        </div>

      {/if}
    </section>
  {/if}
</main>
