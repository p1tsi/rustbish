import init, { process_files } from "./pkg/rustbish.js";

await init(); // Initializes WASM module

document.getElementById('goButton').addEventListener('click', async () => {
  const mainFileInput = document.getElementById('mainFile');
  const walFileInput = document.getElementById('walFile');
  const output = document.getElementById('output');
  const error = document.getElementById('error');
  const time = document.getElementById('time');

  const mainFile = mainFileInput.files[0];
  if (mainFile == null){
    error.textContent = `No SQLite selected`;
    error.style.display = 'block';
    error.style.color = 'red';
  }
  else{
    time.style.display = 'block';
    time.textContent = `Processing...`;

    if (mainFile != null){
      const walFile = walFileInput.files[0];
      const walBuffer = new Uint8Array(await walFile.arrayBuffer());
    }
    
    const mainFileBuffer = new Uint8Array(await mainFile.arrayBuffer());
    const start = performance.now();
    const result = process_files(mainFileBuffer);
    const elapsed = performance.now() - start;

    if (result == "ERROR"){
      error.textContent = `I am sorry but an error occurred...`;
      error.style.display = 'block';
      error.style.color = 'red';
    }
    else{
      time.style.display = 'block';
      time.textContent = `Execution time: ${elapsed.toFixed(3)} ms`;
      output.style.display = 'block';
      output.textContent = `${result.substring(0, 30)}...\n`;
      const userConfirmed = window.confirm(`Do you want to download the result as a JSON file?`);
      if (userConfirmed) {
        const blob = new Blob([result], { type: "application/json" });
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        a.download = "rustbish_output.json";
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        URL.revokeObjectURL(url);
      }
    }
  }
  
  mainFileInput.value = '';
  walFileInput.value = '';
  setTimeout(() => {
    error.style.display = 'none';
    time.style.display = 'none';
    output.style.display = 'none';
    output.textContent = '';
  }, 5000);

});

