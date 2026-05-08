import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import App from './App';
import './i18n'; // initialises i18next; App.tsx switches language from settings.
import './styles/globals.css';

// Suppress the webview's default right-click menu globally — in dev it
// shows "Inspect element", which leaks into custom context menus the
// app builds (e.g. Config card right-click). Radix-based ContextMenu
// uses React's synthetic event which fires regardless of native
// preventDefault, so this doesn't break anything app-side.
window.addEventListener('contextmenu', (e) => {
  e.preventDefault();
});

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>
);
