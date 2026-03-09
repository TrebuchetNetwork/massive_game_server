git clone https://github.com/google/flatbuffers.git
cd flatbuffers
git checkout v25.2.10
cmake -B build -G "Unix Makefiles"
cmake --build build
sudo cmake --install build
apt  install cmake
   68  sudo apt  install cmake
   69  cmake -B build -G "Unix Makefiles"
   70  sudo apt update
   71  sudo apt install build-essential
   72  cmake -B build -G "Unix Makefiles"
   73  cmake --build build
   74  sudo cmake --install build
   75  cmake --build build
   76  sudo cmake --install build
   77  flatc --version
   78  rm -rf build
   79  cmake -B build -G "Unix Makefiles"
   80  cmake --build build --verbose
   81  cmake --install build
   82  sudo cmake --install build
   83  flatc --version
   84  /usr/local/bin/flatc --version
   85  sudo rm /usr/bin/flatc
   86  echo 'export PATH=/usr/local/bin:$PATH' >> ~/.bashrc
   87  source ~/.bashrc
   88  flatc --version