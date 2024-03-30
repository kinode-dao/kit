import { create } from 'zustand'
import { persist, createJSONStorage } from 'zustand/middleware'
import KinoFile from '../types/KinoFile'
import KinodeApi from '@kinode/client-api'
import { TreeFile } from '../types/TreeFile'
import { trimPathToParentFolder } from '../utils/file'
import { Permissions } from '../types/Permissions'

export interface FileTransferStore {
  handleWsMessage: (message: string) => void
  files: KinoFile[]
  setFiles: (files: KinoFile[]) => void
  set: (partial: FileTransferStore | Partial<FileTransferStore>) => void
  filesInProgress: { [key: string]: number }
  setFilesInProgress: (filesInProgress: { [key: string]: number }) => void
  api: KinodeApi | null
  setApi: (api: KinodeApi) => void
  refreshFiles: () => void
  knownNodes: string[]
  setKnownNodes: (knownNodes: string[]) => void
  onAddFolder: (root: string, createdFolderName: string, callback: () => void) => void
  onMoveFile: (file: TreeFile, dest: TreeFile) => void
  errors: string[]
  setErrors: (errors: string[]) => void
  clearErrors: () => void
  permissionsModalOpen: boolean
  setPermissionsModalOpen: (permissionsModalOpen: boolean) => void
  editingPermissionsForPath: string
  setEditingPermissionsForPath: (editingPermissionsForPath: string) => void
  onChangePermissionsForNode: (path: string, perm?: { node: string, allow?: boolean }) => void
  permissions: Permissions
  setPermissions: (permissions: Permissions) => void
}

type WsMessage =
  | { kind: 'progress', data: { name: string, progress: number } }
  | { kind: 'uploaded', data: { name: string, size: number } }

const useFileTransferStore = create<FileTransferStore>()(
  persist(
    (set, get) => ({
      files: [],
      filesInProgress: {},
      knownNodes: [],
      errors: [],
      clearErrors: () => set({ errors: [] }),
      setKnownNodes: (knownNodes) => set({ knownNodes }),
      setErrors: (errors) => set({ errors }),
      api: null,
      setApi: (api) => set({ api }),
      setFilesInProgress: (filesInProgress) => set({ filesInProgress }),
      permissionsModalOpen: false,
      setPermissionsModalOpen: (permissionsModalOpen: boolean) => set({ permissionsModalOpen }),
      editingPermissionsForPath: '',
      setEditingPermissionsForPath: (editingPermissionsForPath: string) => set({ editingPermissionsForPath }),
      permissions: {} as Permissions,
      setPermissions: (permissions: Permissions) => set({ permissions }),
      setFiles: (files) => set({ files }),    
      handleWsMessage: (json: string | Blob) => {
        const { setPermissions, filesInProgress, setFilesInProgress, setKnownNodes, refreshFiles, setErrors, errors } = get()
        if (typeof json === 'string') {
          try {
            console.log('WS: GOT MESSAGE', json)
            const { kind, data } = JSON.parse(json) as WsMessage;
            if (kind === 'progress') {
              const { name, progress } = data
              const fip = { ...filesInProgress, [name]: progress }
              console.log({ fip })
              setFilesInProgress(fip)
              if (progress >= 100) {
                get().refreshFiles()
              }
            } else if (kind === 'uploaded') {
              refreshFiles()
            } else if (kind === 'file_update') {
              refreshFiles()
            } else if (kind === 'state') {
              const { known_nodes, permissions } = data
              setKnownNodes(known_nodes)
              setPermissions(permissions)
            } else if (kind === 'error') {
              console.log({ error: data })
              setErrors([...errors, data])
            }
          } catch (error) {
            console.error("Error parsing WebSocket message", error);
          }
        } else {
            console.log('WS: GOT BLOB', json)
        }
      },
      onAddFolder: (root: string, createdFolderName: string, callback: () => void) => {
        const { api } = get();
        if (!api) return alert('No API');
        if (!createdFolderName) return alert('No folder name');
        if (!window.confirm(`Are you sure you want to add ${createdFolderName}?`)) return;

        api.send({
            data: {
                CreateDir: {
                    name: `${root}/${createdFolderName}`
                }
            }
        })

        callback()
      },
      onMoveFile: ({ file }: TreeFile, { file: dest }: TreeFile) => {
        const { api, refreshFiles } = get();
        console.log('moving file', file.name, dest.name);
        if (!api) return alert('No API');
        if (!file.name) return alert('No file name');
        if (!dest.name) return alert('No destination name');
        if (!dest.dir) return alert('No destination directory');
        if (trimPathToParentFolder(file.name) === dest.name) return;
        if (file.name === dest.name) return alert('Cannot move a file in-place');
        if (!window.confirm(`Are you sure you want to move ${file.name} to ${dest.name}?`)) return;

        api.send({
            data: {
                Move: {
                    source_path: file.name,
                    target_path: dest.name
                }
            }
        })

        setTimeout(() => refreshFiles(), 1000);
      },
      onChangePermissionsForNode: (path: string, perm?: { node: string, allow?: boolean }) => {
        const { api, refreshFiles } = get()
        console.log('changing node access to file', path, perm);
        if (!api) return alert('No API');
        if (!path) return alert('No file name');
        if (perm && perm.allow !== undefined) {
          if (!window.confirm(`Are you sure you want to ${perm.allow ? 'allow' : 'forbid'} ${perm.node} to access ${path}?`)) return;
        } else if (perm && perm.node) {
          if (!window.confirm('Are you sure you want to remove this permission?')) return;
        } else {
          if (!window.confirm(`Are you sure you want to remove all permissions for ${path}?`)) return;
        }

        api.send({ data: { ChangePermissions: { path, perm } } })

        setTimeout(() => refreshFiles(), 1000);
      },
      refreshFiles: () => {
        const { setFiles } = get()
        console.log('refreshing files')
        fetch(`${import.meta.env.BASE_URL}/files`)
          .then((response) => response.json())
          .then((data) => {
            try {
              setFiles(data.ListFiles)
            } catch {
              console.log("Failed to parse JSON files", data);
            }
          })
      },
      set,
      get,
    }),


    {
      name: 'kino_files', // unique name
      storage: createJSONStorage(() => sessionStorage), // (optional) by default, 'localStorage' is used
    }
  )
)

export default useFileTransferStore