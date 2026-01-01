// Main entry point for BookLore launcher UI
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// Status elements
const statusMariadb = document.getElementById('status-mariadb');
const statusJre = document.getElementById('status-jre');
const statusBackend = document.getElementById('status-backend');
const progressBar = document.getElementById('progress');
const errorContainer = document.getElementById('error-container');
const errorText = document.getElementById('error-text');

// Check if we are in shutdown mode
const urlParams = new URLSearchParams(window.location.search);
const isShutdown = urlParams.get('shutdown') === 'true';

// Status states
const STATUS = {
  PENDING: 'pending',
  ACTIVE: 'active',
  COMPLETE: 'complete',
  ERROR: 'error'
};

function setStatus(element, status, message) {
  // Remove all status classes
  element.classList.remove('active', 'complete', 'error');

  // Add new status class
  if (status !== STATUS.PENDING) {
    element.classList.add(status);
  }

  // Update icon
  const iconContainer = element.querySelector('.status-icon');
  if (status === STATUS.PENDING) {
    iconContainer.innerHTML = '<span class="pending-icon">○</span>';
  } else if (status === STATUS.ACTIVE) {
    iconContainer.innerHTML = '<div class="spinner"></div>';
  } else if (status === STATUS.COMPLETE) {
    iconContainer.innerHTML = '<span class="checkmark">✓</span>';
  } else if (status === STATUS.ERROR) {
    iconContainer.innerHTML = '<span class="error-icon">✕</span>';
  }

  // Update message if provided
  if (message) {
    element.querySelector('.status-text').textContent = message;
  }
}

function setProgress(percent) {
  progressBar.style.width = `${percent}%`;
}

function showError(message) {
  errorText.textContent = message;
  errorContainer.classList.add('visible');
}

// Listen for status updates from Tauri backend
async function initializeApp() {
  try {
    // Listen for startup events from Rust backend
    await listen('startup-status', (event) => {
      const { stage, status, message, progress } = event.payload;

      let element;
      switch (stage) {
        case 'mariadb':
          element = statusMariadb;
          break;
        case 'jre':
          element = statusJre;
          break;
        case 'backend':
          element = statusBackend;
          break;
      }

      if (element) {
        setStatus(element, status, message);
      }

      if (progress !== undefined) {
        setProgress(progress);
      }

      // If backend is complete, navigate webview to BookLore UI
      // Uses the frontend HTTP server (port 18088) which serves Angular and proxies /api to backend
      if (stage === 'backend' && status === STATUS.COMPLETE) {
        console.log('All services ready! Navigating to BookLore UI...');
        setTimeout(() => {
          // Navigate the current webview to the frontend server
          window.location.href = 'http://localhost:18088';
        }, 1000);
      }

      // Handle errors
      if (status === STATUS.ERROR) {
        showError(message);
      }
    });

    // Listen for file drops directly on the webview
    await listen('tauri://drop', async (event) => {
      const { paths } = event.payload;
      if (paths && paths.length > 0) {
        console.log('Files dropped:', paths);
        try {
          const count = await invoke('handle_dropped_files', { files: paths });
          console.log(`Successfully imported ${count} files`);
          // Show a temporary success message?? For now, just log it.
          // In a full app we'd show a toast notification
        } catch (e) {
          console.error('Failed to handle dropped files:', e);
          showError(`Import failed: ${e}`);
        }
      }
    });

    // Listen for shutdown events
    await listen('shutdown-start', () => {
      console.log('Shutdown sequence initiated');
      document.querySelector('h1').textContent = 'Stopping BookLore...';

      // Reset statuses
      setStatus(statusBackend, STATUS.ACTIVE, 'Stopping backend...');
      setStatus(statusJre, STATUS.PENDING, 'Waiting...');
      setStatus(statusMariadb, STATUS.PENDING, 'Waiting...');
      setProgress(0);
      errorContainer.classList.remove('visible');
    });

    await listen('shutdown-status', (event) => {
      const { stage, status, message, progress } = event.payload;

      let element;
      switch (stage) {
        case 'backend':
          element = statusBackend;
          break;
        case 'jre': // We might not need to stop JRE explicitly if it's a child process, but nice to show
          element = statusJre;
          break;
        case 'mariadb':
          element = statusMariadb;
          break;
      }

      if (element) {
        setStatus(element, status, message);
      }
      if (progress !== undefined) {
        setProgress(progress);
      }
    });

    // Start the initialization process ONLY if we are not shutting down
    // (The window might reload during shutdown if we navigate back to it)
    if (!isShutdown) {
      await invoke('start_services');
    } else {
      // Initialize UI for shutdown immediately
      console.log('Shutdown mode detected');
      document.querySelector('h1').textContent = 'Stopping BookLore...';
      setStatus(statusBackend, STATUS.ACTIVE, 'Stopping backend...');
      setStatus(statusJre, STATUS.PENDING, 'Waiting...');
      setStatus(statusMariadb, STATUS.PENDING, 'Waiting...');
      setProgress(0);
    }
  } catch (error) {
    console.error('Failed to initialize:', error);
    showError(`Failed to start: ${error}`);
  }
}

// Start when Tauri is ready
if (window.__TAURI__) {
  initializeApp();
} else {
  // Development mode without Tauri
  console.log('Running in development mode (no Tauri)');

  // Simulate startup sequence for testing
  async function simulateStartup() {
    setStatus(statusMariadb, STATUS.ACTIVE, 'Starting database...');
    setProgress(10);

    await new Promise(r => setTimeout(r, 1500));
    setStatus(statusMariadb, STATUS.COMPLETE, 'Database ready');
    setStatus(statusJre, STATUS.ACTIVE, 'Checking Java runtime...');
    setProgress(40);

    await new Promise(r => setTimeout(r, 1000));
    setStatus(statusJre, STATUS.COMPLETE, 'Java runtime ready');
    setStatus(statusBackend, STATUS.ACTIVE, 'Starting BookLore backend...');
    setProgress(70);

    await new Promise(r => setTimeout(r, 2000));
    setStatus(statusBackend, STATUS.COMPLETE, 'BookLore is ready!');
    setProgress(100);
  }

  simulateStartup();
}
