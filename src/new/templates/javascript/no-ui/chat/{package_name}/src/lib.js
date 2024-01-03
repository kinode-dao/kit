import { printToTerminal, receive, sendAndAwaitResponse, sendResponse } from "uqbar:process/standard@0.5.0";

function parseAddress(addressString) {
    const [node, rest] = addressString.split('@');
    const [process, packageName, publisher] = rest.split(':');
    return { node, process, packageName, publisher };
}

function inputBytesToString(byteObject) {
    // Determine the size of the Uint8Array
    const size = Object.keys(byteObject).length;
    const byteArray = new Uint8Array(size);

    // Assign the bytes to the array
    for (let i = 0; i < size; i++) {
        byteArray[i] = byteObject[i];
    }

    // Convert the Uint8Array to a string
    const string = new TextDecoder().decode(byteArray);

    return {bytes: byteArray, string: string};
}

function handleMessage(ourNode, messageArchive) {
    const [source, message] = receive();

    if (message.tag == 'response') {
        printToTerminal(0, `{package_name}: unexpected Response: ${JSON.stringify(message.val)}`);
        process.exit(1);
    } else if (message.tag == 'request') {
        const { bytes: ipcBytes, string: ipc0 } = inputBytesToString(message.val.ipc)
        const ipc = JSON.parse(ipc0);
        const encoder = new TextEncoder();
        if (ipc.Send) {
            const { target, message: messageText } = ipc.Send;
            if (target === ourNode) {
                printToTerminal(0, `{package_name}|${source.node}: ${messageText}`);
                messageArchive[source.node] = messageText;
            } else {
                sendAndAwaitResponse(
                    {
                        node: target,
                        process: {
                            processName: "{package_name}",
                            packageName: "{package_name}",
                            publisherNode: "{publisher}"
                        }
                    },
                    {
                        inherit: false,
                        expectsResponse: 5,
                        ipc: ipcBytes,
                        metadata: null
                    },
                    null
                );
            }
            sendResponse(
                {
                    inherit: false,
                    ipc: encoder.encode(JSON.stringify({ Ack: null })),
                    metadata: null
                },
                null
            );
        } else if (ipc.History) {
            sendResponse(
                {
                    inherit: false,
                    ipc: encoder.encode(JSON.stringify({ History: { messages: messageArchive } })),
                    metadata: null
                },
                null
            );
        } else {
            process.exit(1);
        }
    }
    return messageArchive;
}
export function init(our) {
    printToTerminal(0, `{package_name}: begin (javascript)`);

    const { node: ourNode } = parseAddress(our);
    let messageArchive = {};

    while (true) {
        try {
            messageArchive = handleMessage(ourNode, messageArchive);
        } catch (error) {
            printToTerminal(0, `{package_name}: got error ${error}`);
        }
    }
}
