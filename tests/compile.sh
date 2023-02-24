echo "Test: It should compile"
cd ../testrepo

if ../target/release/rewatch clean . &> /dev/null;
then
  echo "✅ - Repo Cleaned"
else 
  echo "❌ - Error Cleaning Repo"
  exit 1
fi

if ../target/release/rewatch build . &> /dev/null; 
then
  echo "✅ - Repo Built"
else 
  echo "❌ - Error Building Repo"
  exit 1
fi

if git diff --exit-code ./ &> /dev/null; 
then
  echo "✅ - Testrepo has no changes"
else 
  echo "❌ - Build has changed"
  exit 1
fi

if node ../testrepo/packages/main/src/Main.mjs | grep -z '01\n02\n03' &> /dev/null; 
then
  echo "✅ - Output is correct"
else 
  echo "❌ - Output is incorrect"
  exit 1
fi
