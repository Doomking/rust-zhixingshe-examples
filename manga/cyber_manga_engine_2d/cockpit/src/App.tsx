import { useState } from 'react';
import './App.css';

function App() {
  const [prompt, setPrompt] = useState('A cyberpunk city under rain, anime style');
  const [mode, setMode] = useState<'image' | 'manga'>('image');
  const [generating, setGenerating] = useState(false);
  const [imageUrl, setImageUrl] = useState<string | null>(null);
  const [videoUrl, setVideoUrl] = useState<string | null>(null);
  const [panels, setPanels] = useState<any[]>([]);
  const [errorUrl, setError] = useState<string | null>(null);

  const generateImage = async () => {
    if (!prompt) return;
    setGenerating(true);
    setError(null);
    setImageUrl(null);
    setVideoUrl(null);
    setPanels([]);

    const endpoint = mode === 'image' ? '/api/generate' : '/api/generate_manga';
    const payload = mode === 'image' ? { prompt } : { script: prompt };

    try {
      const response = await fetch(`http://localhost:3000${endpoint}`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify(payload),
      });

      if (!response.ok) {
        throw new Error(`Error: ${response.statusText}`);
      }

      if (mode === 'image') {
        const data = await response.json();
        setImageUrl(data.image);
        setPanels([]);
      } else {
        const data = await response.json();
        setImageUrl(data.image);
        setVideoUrl(data.video); // Set video state
        setPanels(data.panels);
      }

    } catch (err: any) {
      console.error(err);
      setError(err.message || 'Failed to generate image');
    } finally {
      setGenerating(false);
    }
  };

  const openPlayer = (imgUrl: string, vidUrl?: string | null) => {
    const player = document.getElementById('video-player');
    const img = document.getElementById('projector-img') as HTMLImageElement;

    if (player) {
      // If we have video, relying on React State to render <video>, but we need to show modal
      player.style.display = 'flex';

      // Fallback for image only mode if needed
      if (img && !vidUrl) {
        img.src = imgUrl;
      }

      // Start Narration (Dubbing for the video)
      if (panels.length > 0) {
        speakScript(panels);
      }
    }
  };

  const closePlayer = () => {
    const player = document.getElementById('video-player');
    if (player) player.style.display = 'none';
    window.speechSynthesis.cancel();
  };

  const speakScript = (scriptPanels: any[]) => {
    window.speechSynthesis.cancel();
    let delay = 1000;

    scriptPanels.forEach((panel) => {
      setTimeout(() => {
        const utterance = new SpeechSynthesisUtterance(panel.dialogue);
        // Simple voice heuristic
        const voices = window.speechSynthesis.getVoices();
        // Try to find specific voices if available, else standard
        // (Optional: filter voices by lang if needed)

        if (panel.role.toLowerCase().includes('girl')) {
          utterance.pitch = 1.2;
          utterance.rate = 1.1;
        } else if (panel.role.toLowerCase().includes('robot')) {
          utterance.pitch = 0.5;
          utterance.rate = 0.9;
        }
        window.speechSynthesis.speak(utterance);
      }, delay);

      // Estimate duration: ~200ms per char + buffer
      delay += (panel.dialogue.length * 200) + 1500;
    });
  };

  return (
    <div className="container">
      <h1>CyberManga Engine</h1>
      <div className="card">
        <div className="mode-toggle">
          <button
            className={`toggle-btn ${mode === 'image' ? 'active' : ''}`}
            onClick={() => { setMode('image'); setPrompt('A cyberpunk city under rain, anime style'); }}
          >
            Single Image
          </button>
          <button
            className={`toggle-btn ${mode === 'manga' ? 'active' : ''}`}
            onClick={() => { setMode('manga'); setPrompt('CyberGirl: Within the simulation, rain is meaningless.\nRobot: Yet you still hold an umbrella.'); }}
          >
            Manga Strip
          </button>
        </div>

        <div className="input-group">
          <textarea
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            placeholder={mode === 'image' ? "Enter your prompt here..." : "Enter script (Role: Dialogue)..."}
            rows={mode === 'manga' ? 8 : 4}
            className="prompt-input"
          />
        </div>
        <button
          onClick={generateImage}
          disabled={generating}
          className="generate-btn"
        >
          {generating ? 'Generating (This may take a while)...' : (mode === 'image' ? 'Generate Image' : 'Generate Manga Strip')}
        </button>

        {errorUrl && <p className="error">{errorUrl}</p>}
      </div>

      <div className="result-area">
        {mode === 'manga' && videoUrl ? (
          <div className="video-container" style={{ textAlign: 'center', width: '100%' }}>
            <video
              src={videoUrl}
              controls
              autoPlay
              className="generated-video"
              style={{ maxWidth: '100%', maxHeight: '80vh', border: '2px solid #0ff', boxShadow: '0 0 20px #0ff' }}
            />
            <div className="action-buttons" style={{ marginTop: '10px' }}>
              <a href={videoUrl} download="manga.mp4" className="download-link">Download Video</a>
            </div>
          </div>
        ) : imageUrl ? (
          <div className="image-container">
            <img src={imageUrl} alt="Generated" className="generated-image" />
            <div className="action-buttons">
              <a href={imageUrl} download="generated.png" className="download-link">Download Image</a>
            </div>
          </div>
        ) : (
          <div className="placeholder">
            {generating ? <div className="loader">Creating Art...</div> : <p>Your creation will appear here</p>}
          </div>
        )}
      </div>
    </div>
  );
}

export default App;
