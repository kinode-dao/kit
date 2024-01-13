import { printToTerminal, receive, sendAndAwaitResponse, sendResponse } from "nectar:process/standard@0.7.0";

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
        throw new Error(`unexpected Response: ${JSON.stringify(message.val)}`);
    } else if (message.tag == 'request') {
        const { bytes: bodyBytes, string: body0 } = inputBytesToString(message.val.body)
        const body = JSON.parse(body0);
        const encoder = new TextEncoder();
        if (body.Send) {
            const { target, message: messageText } = body.Send;
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
                        body: bodyBytes,
                        metadata: null
                    },
                    null
                );
            }
            sendResponse(
                {
                    inherit: false,
                    body: encoder.encode(JSON.stringify({ Ack: null })),
                    metadata: null
                    capabilities: [],
                },
                null
            );
        } else if (body.History) {
            sendResponse(
                {
                    inherit: false,
                    body: encoder.encode(JSON.stringify({ History: { messages: messageArchive } })),
                    metadata: null,
                    capabilities: [],
                },
                null
            );
        } else {
            throw new Error(`Unexpected Request: ${body}`)
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
