import { useState } from "react";
import useFileTransferStore from "../store/fileTransferStore";
import { FaX } from "react-icons/fa6";

const UploadFiles = () => {
  const [filesToUpload, setFilesToUpload] = useState<File[]>([])
  const { refreshFiles } = useFileTransferStore();

  const onAddFiles = (event: React.ChangeEvent<HTMLInputElement>) => {
    if (event.target.files) {
      setFilesToUpload(Array.from(event.target.files))
    }
  }

  const onUploadFiles = () => {
    const formData = new FormData()
    filesToUpload.forEach((file) => {
      formData.append('files', file)
    })

    fetch(`${import.meta.env.BASE_URL}/files`, {
      method: 'POST',
      body: formData,
    })
      .then(() => {
        refreshFiles()
        setFilesToUpload([])
      })
      .catch((err) => {
        alert(err)
      })
  }

  const onRemoveFileToUpload = (file: File) => {
    if (!window.confirm(`Are you sure you want to remove ${file.name}?`)) return
    setFilesToUpload((files) => files.filter((f) => f !== file))
  }

  return (
    <div className='flex place-content-center place-items-center px-2 py-1'>
      <h3 className='text-xl font-bold px-2 py-1'>Upload</h3>
      <div className='flex flex-col px-2 py-1'>
        {filesToUpload.length === 0 && <label htmlFor='files' className='bg-blue-500 hover:bg-blue-700 font-bold py-2 px-4 rounded cursor-pointer text-center'>
          Choose Files
          <input id='files' type='file' hidden multiple onChange={onAddFiles} />
        </label>}

        {filesToUpload.length > 0 && (
          <div className='flex flex-col px-2 py-1'>
            <ul>
              {filesToUpload.map((file) => (
                <li 
                  key={file.name}
                  className="flex place-items-center bg-gray-800 hover:bg-gray-700/50 font-bold py-1 px-2 rounded cursor-pointer"
                  onClick={() => onRemoveFileToUpload(file)}
                >{file.name} <FaX className="ml-auto pl-1" /></li>
              ))}
            </ul>
            <span>{filesToUpload.length} files selected</span>
            <span>Total: {filesToUpload.reduce((acc, file) => acc + file.size, 0)} bytes</span>
            <div className="flex flex-row grow">
              <button className='bg-white/10 grow hover:bg-red-500 font-bold py-2 px-4 rounded' onClick={() => setFilesToUpload([])}>
                Clear
              </button>
              <button className='bg-blue-500 grow hover:bg-blue-700 font-bold py-2 px-4 rounded' onClick={onUploadFiles}>
                Upload
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  )
}

export default UploadFiles;