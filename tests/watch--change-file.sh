echo "Test: It should watch"
cd ../testrepo

if RUST_BACKTRACE=1 ../target/release/rewatch clean . &> /dev/null;
then
  echo "✅ - Repo Cleaned"
else 
  echo "❌ - Error Cleaning Repo"
  exit 1
fi

RUST_BACKTRACE=1 ../target/release/rewatch watch . &>/dev/null &
echo "✅ - Watcher Started"

echo 'Js.log("added-by-test")' >> ./packages/main/src/Main.res

sleep 1

if node ./packages/main/src/Main.mjs | grep 'added-by-test' &> /dev/null; 
then
  echo "✅ - Output is correct"
else 
  echo "❌ - Output is incorrect"
  exit 1
fi

sleep 1

if [[ $OSTYPE == 'darwin'* ]]; 
then
  sed -i '' '/Js.log("added-by-test")/d' ./packages/main/src/Main.res;
else 
  sed -i '/Js.log("added-by-test")/d' ./packages/main/src/Main.res;
fi

sleep 2

if git diff --exit-code ./ &> /dev/null; 
then
  echo "✅ - Adding and removing changes nothing"
else 
  echo "❌ - Adding and removing changes left some artifacts"
  exit 1
fi
