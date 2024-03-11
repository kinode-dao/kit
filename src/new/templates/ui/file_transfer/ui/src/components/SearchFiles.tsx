import { useEffect, useState } from 'react';
import KinoFile from '../types/KinoFile';
import FileEntry from './FileEntry';
import useFileTransferStore from '../store/fileTransferStore';
import SortableTree, { TreeItem, toggleExpandedForAll } from '@nosferatu500/react-sortable-tree';
import FileExplorerTheme from '@nosferatu500/theme-file-explorer';

const SearchFiles = function() {
    const { knownNodes, setKnownNodes } = useFileTransferStore();
    const [searchTerm, setSearchTerm] = useState('');
    const [foundFiles, setFoundFiles] = useState<KinoFile[] | undefined>();
    const [searching, setSearching] = useState<boolean>(false);
    const [expandedFiles, setExpandedFiles] = useState<{ [path:string]: boolean }>({})
    const [treeData, setTreeData] = useState<TreeItem[]>([])

    const handleSearch = () => {
        if (!searchTerm) return alert('Please enter a node name.');
        if (!searchTerm.match(/^[a-zA-Z0-9-]+\.os$/)) return alert('Invalid node name.');
        setKnownNodes([...knownNodes, searchTerm].filter((v, i, a) => a.indexOf(v) === i));
        setSearching(true);
        try {
            fetch(`${import.meta.env.BASE_URL}/files?node=${searchTerm}`, {
                method: 'GET',
                headers: {
                    'Content-Type': 'application/json',
                },
            }).then((response) => response.json())
            .catch(() => {
                window.alert(`${searchTerm} appears to be offline, or has not installed Kino Files.`)
                setSearching(false);
            })
            .then((data) => {
                try {
                    setFoundFiles(data.ListFiles)
                    setSearching(false);
                } catch {
                    console.log("Failed to parse JSON files", data);
                }
            });
        } catch (error) {
            console.error('Error:', error);
        }
    };

    const treeifyFile: (node: string, f: KinoFile) => TreeItem = (node: string, file: KinoFile) => {
        return {
            title: <FileEntry file={file} node={node} isOurFile={false} />,
            children: file.dir ? file.dir.map((f: KinoFile) => treeifyFile(node,f)) : undefined,
            file,
            expanded: !!expandedFiles[file.name]
        } as TreeItem;
    }

    const expand = (expanded: boolean) => {
        setTreeData(toggleExpandedForAll({ treeData, expanded }))
        setExpandedFiles((prev) => ({ ...prev, ...treeData.reduce((acc, node) => ({ ...acc, [node.file.name]: expanded }), {}) }))
    }

    useEffect(() => {
        if (foundFiles) {
            const td = foundFiles.map(file => treeifyFile(searchTerm, file))
            setTreeData(td || [])
        }
    }, [foundFiles])

    return (
        <div className='flex flex-col px-2 py-1 grow'>
            <h2 className='text-xl mb-2 font-bold'>Search files on the network</h2>
            <div className='flex place-items-center'>
                <div className='flex grow place-items-center mb-2'>
                    <span className='mr-2'>Node:</span>
                    <input
                        className='bg-gray-800 appearance-none border-2 border-gray-800 rounded w-full py-2 px-4 text-white leading-tight focus:outline-none focus:bg-gray-800 focus:border-blue-500'
                        type="text"
                        value={searchTerm}
                        placeholder='somenode.os'
                        disabled={searching}
                        onChange={(e) => setSearchTerm(e.target.value)}
                    />
                    <button
                        className='bg-blue-500 hover:bg-blue-700 text-white font-bold py-2 px-4 rounded'
                        onClick={handleSearch}
                        disabled={searching}
                    >
                        {searching ? 'Searching...' : 'Search'}
                    </button>
                </div>
                {knownNodes.length > 0 && <div className='flex grow place-items-center mb-2'>
                    <span className='mx-2'>or:</span>
                    <select
                        className='bg-gray-800 appearance-none border-2 border-gray-800 rounded w-full py-2 px-4 text-white leading-tight focus:outline-none focus:bg-gray-800 focus:border-blue-500'
                        onChange={(e) => setSearchTerm(e.target.value)}
                    >
                        <option value=''>Select a known node</option>
                        {knownNodes.map((node) => (
                            <option key={node} value={node}>{node}</option>
                        ))}
                    </select>
                </div>}
            </div>
            {searching && <span className='text-white'>Searching...</span>}
            {!searching && !foundFiles && <span className='text-white'>Enter a node name to search for files.</span>}
            {!searching && foundFiles && <h2 className='text-xl font-bold flex place-items-center'>
                Search Results
                <button className='rounded px-2 py-1 ml-4 bg-white/10 text-sm'
                    onClick={() => expand(true)}
                >
                    Expand All
                </button>
                <button className='rounded px-2 py-1 ml-4 bg-white/10 text-sm'
                    onClick={() => expand(false)}
                >
                    Collapse All
                </button>
            </h2>}
            {!searching && foundFiles && foundFiles.length === 0 && <span className='text-white'>No files found.</span>}
            {foundFiles && foundFiles.length > 0 && <div className='flex flex-col px-2 py-1 grow'>
                <h2>
                    <span className='text-xl font-bold font-mono'>{searchTerm}:</span> <span className='text-xs'>{foundFiles.length} files</span>
                </h2>
                <div className='grow overflow-y-auto'>
                    <SortableTree
                        theme={FileExplorerTheme}
                        treeData={treeData}
                        onChange={treeData => setTreeData([...treeData])}
                        getNodeKey={({ node }: { node: TreeItem }) => node.file.name}
                        onVisibilityToggle={({ expanded, node }) => {
                            setExpandedFiles((prev) => ({ ...prev, [node?.file?.name]: expanded }))
                        }}
                        canDrag={() => false}
                        canDrop={() => false}
                    />
                </div>
            </div>}
        </div>
    );
};

export default SearchFiles;
