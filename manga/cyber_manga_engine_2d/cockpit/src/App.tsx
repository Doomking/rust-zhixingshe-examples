import { useState } from 'react';
import './App.css';

function App() {
  const [prompt, setPrompt] = useState('有个叫米奥的女孩，大概16岁吧，在个破旧的工坊里修东西。阳光很好。她突然很生气，因为零件卡住了，还冒了一股烟。她大喊说："气死我了，又坏了！"');
  const [generating, setGenerating] = useState(false);
  const [imageUrl, setImageUrl] = useState<string | null>(null);
  const [videoUrl, setVideoUrl] = useState<string | null>(null);
  const [panels, setPanels] = useState<any[]>([]);
  const [errorUrl, setError] = useState<string | null>(null);

  const generateManga = async () => {
    if (!prompt) return;
    setGenerating(true);
    setError(null);
    setImageUrl(null);
    setVideoUrl(null);
    setPanels([]);

    try {
      const response = await fetch('http://localhost:3000/api/generate_manga', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({ script: prompt }),
      });

      if (!response.ok) {
        throw new Error(`Error: ${response.statusText}`);
      }

      const data = await response.json();
      setImageUrl(data.image);
      setVideoUrl(data.video);
      setPanels(data.panels);

    } catch (err: any) {
      console.error(err);
      setError(err.message || 'Failed to generate content');
    } finally {
      setGenerating(false);
    }
  };

  return (
    <div className="container">
      <h1>CyberManga Engine</h1>
      <p style={{ color: '#888', marginBottom: '20px', fontSize: '14px' }}>
        AI漫剧引擎 - 输入故事脚本，自动生成动态视频
      </p>
      <div className="main-content">
        <div className="card">
          <div className="input-group">
            <textarea
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              placeholder="输入你的故事脚本...描述角色、场景和对话"
              rows={8}
              className="prompt-input"
            />
          </div>
          
          <div className="controls" style={{ marginBottom: '15px' }}>
             <label style={{ color: '#ccc', marginRight: '10px' }}>画风选择:</label>
             <select className="style-select" style={{ padding: '8px', borderRadius: '4px', background: '#333', color: '#fff', border: '1px solid #555' }}>
               <option value="ghibli">吉卜力 (Ghibli)</option>
               <option value="manga">黑白漫画 (Manga)</option>
               <option value="anime">日系动画 (Anime)</option>
             </select>
          </div>

          <button
            onClick={generateManga}
            disabled={generating}
            className="generate-btn"
          >
            <span>{generating ? 'AI 正在绘制漫画...' : '生成漫剧'}</span>
          </button>

          {errorUrl && <p className="error">{errorUrl}</p>}
        </div>

        <div className="result-area">
          {imageUrl ? (
            <div className="image-container" style={{ textAlign: 'center' }}>
              <h2 style={{ color: '#fff', marginBottom: '10px' }}>生成结果</h2>
              <img 
                src={imageUrl} 
                alt="Generated Manga" 
                className="generated-image" 
                style={{ maxWidth: '100%', borderRadius: '8px', boxShadow: '0 4px 20px rgba(0,0,0,0.5)' }} 
              />
              <div className="action-buttons" style={{ marginTop: '15px' }}>
                <a href={imageUrl} download="manga_page.png" className="download-btn" style={{ padding: '10px 20px', background: '#00ccff', color: '#000', textDecoration: 'none', borderRadius: '4px', fontWeight: 'bold' }}>
                  下载漫画页
                </a>
              </div>
            </div>
          ) : (
            <div className="placeholder">
              {generating ? (
                <div className="loader-container">
                    <div className="loader"></div>
                    <p style={{ marginTop: '10px', color: '#888' }}>正在绘制分镜...</p>
                </div>
              ) : (
                <div className="empty-state">
                    <p>你的漫剧将显示在这里</p>
                </div>
              )}
            </div>
          )}

        </div>
      </div>
    </div>
  );
}

export default App;
