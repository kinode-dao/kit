import { useEffect } from 'react'
import './App.css'
import MyFiles from './components/MyFiles'
import KinodeEncryptorApi from '@kinode/client-api'
import useFileTransferStore from './store/fileTransferStore';
import SearchFiles from './components/SearchFiles';
import UploadFiles from './components/UploadFiles';
import { PermissionsModal } from './components/PermissionsModal';

declare global {
  var window: Window & typeof globalThis;
  var our: { node: string, process: string };
}

let inited = false 

function App() {
  const { files, handleWsMessage, setApi, refreshFiles, permissionsModalOpen, } = useFileTransferStore();

  const BASE_URL = import.meta.env.BASE_URL;
  const PROXY_TARGET = `${(import.meta.env.VITE_NODE_URL || "http://localhost:8080")}${BASE_URL}`;
  const WEBSOCKET_URL = import.meta.env.DEV
  ? `${PROXY_TARGET.replace('http', 'ws')}`
  : undefined;

  if (window.our) window.our.process = BASE_URL?.replace("/", "");

  useEffect(() => {
    if (!inited) {
      inited = true

      const api = new KinodeEncryptorApi({
        uri: WEBSOCKET_URL,
        nodeId: window.our.node,
        processId: window.our.process,
        onMessage: handleWsMessage
      });

      setApi(api);
    }
  }, []) 

  useEffect(() => {
    refreshFiles()
  }, [])


  return (
    <div className='flex text-white'>
      <div className='flex flex-col w-1/2 bg-gray-800 h-screen sidebar'>
        <h2 className='text-2xl px-2 py-1 display self-center'>Kino Files</h2>
        <div className='flex flex-col grow'>
          <MyFiles node={window.our.node} files={files} />
          <UploadFiles />
        </div>
      </div>
      <div className='flex flex-col w-1/2 bg-gray-900 h-screen content px-2 py-1 overflow-y-auto'>
        <SearchFiles />
      </div>
      {permissionsModalOpen && <PermissionsModal />}
    </div>
  )
}

export default App
